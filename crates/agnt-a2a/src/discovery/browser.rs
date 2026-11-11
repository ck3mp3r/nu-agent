use std::thread::JoinHandle;
use std::time::Duration;

use mdns_sd::{ServiceDaemon, ServiceEvent};
use tokio::runtime::Handle;
use tokio::sync::mpsc;

use crate::client::{A2aClient, A2aHttpClient};
use crate::{A2aError, AgentCard, peer::Peer};

use super::filter;
use super::service::{peer_from_service, peer_name_from_fullname};

// ---------------------------------------------------------------------------
// DiscoveryBrowser — mDNS service browsing
// ---------------------------------------------------------------------------

/// The mDNS service type this mesh registers and browses.
pub(crate) const SERVICE_TYPE: &str = "_nu-agent-a2a._tcp.local.";

/// The card-fetch deadline for one browse resolution.
///
/// Per request, never client-global: the transport belongs to the composition
/// root and must stay unbounded for every other caller.
const CARD_FETCH_TIMEOUT: Duration = Duration::from_secs(5);

/// Events emitted by the [`DiscoveryBrowser`] when peers appear or disappear.
#[derive(Debug, Clone)]
pub enum PeerEvent {
    /// A new peer was discovered and resolved.
    PeerDiscovered(Box<Peer>),
    /// An existing peer is no longer available.
    PeerLost(String),
}

/// Browses the local network for A2A agents registered on
/// `_nu-agent-a2a._tcp.local.`.
///
/// Uses the pure-Rust [`ServiceDaemon`] from the `mdns-sd` crate — no C FFI
/// dependency.
pub(crate) struct DiscoveryBrowser {
    // NOTE: field order matters for Drop.  The manual Drop impl releases this
    // daemon clone, then joins the poll thread.  Dropping the clone does not
    // close the browse channel — that channel closes when the querier goes
    // away: `stop_browse` removes it and purges its pending retransmissions
    // (the rediscover path), and daemon shutdown does the same.  The daemon
    // field is *last* so that if Rust's auto-drop runs after the manual Drop,
    // the daemon outlives the thread.
    poll_thread: Option<JoinHandle<()>>,
    _daemon: Option<ServiceDaemon>,
}

impl DiscoveryBrowser {
    /// Start browsing for A2A agents, using a shared daemon.
    ///
    /// The daemon is shared with [`DiscoveryService`] so that both
    /// registration and browsing go through a single mDNS responder.
    ///
    /// Returns a handle that can be dropped to stop browsing, and a
    /// `mpsc::Receiver` that yields [`PeerEvent`]s as peers are discovered
    /// or lost.
    ///
    /// `card_client` and `runtime_handle` are handed down from the composition
    /// root: the shared transport and the handle of the runtime that owns this
    /// agent. The poll thread is a bare `std::thread`, so `Handle::block_on` is
    /// how it enters that runtime to fetch a card. The owning runtime must stay
    /// alive — and driven — until every browse thread holding a handle has
    /// exited: the CLI and plugin binaries build one runtime per process and
    /// keep it, so a library user must do the same.
    pub fn browse(
        daemon: ServiceDaemon,
        mesh_key: &str,
        own_port: u16,
        card_client: A2aClient,
        runtime_handle: Handle,
    ) -> Result<(Self, mpsc::Receiver<PeerEvent>), A2aError> {
        let receiver = daemon
            .browse(SERVICE_TYPE)
            .map_err(|e| A2aError::Internal(format!("mdns-sd browse: {e}")))?;

        let (tx, rx) = mpsc::channel::<PeerEvent>(64);
        let cb_tx = tx.clone();
        let mesh_key = mesh_key.to_string();
        let filters = filter::build_filters(own_port, &mesh_key);

        let handle = std::thread::spawn(move || {
            // recv() blocks until an event arrives. recv() returns Err when the
            // browse channel closes: `stop_browse` removes the querier and purges
            // its pending retransmissions (the rediscover path), and daemon
            // shutdown does the same.
            while let Ok(service_event) = receiver.recv() {
                match service_event {
                    ServiceEvent::ServiceResolved(resolved) => {
                        // Apply the filter chain (self-exclusion, mesh scoping, etc.).
                        if let Err(rejection) = filter::check_all(
                            &filters,
                            resolved.get_fullname(),
                            resolved.get_properties(),
                            resolved.get_port(),
                            resolved
                                .get_addresses()
                                .iter()
                                .any(|addr| addr.is_loopback()),
                        ) {
                            log::debug!("peer rejected: {rejection}");
                            continue;
                        }

                        let mut peer = peer_from_service(&resolved);
                        let url = format!("{}/.well-known/agent-card.json", peer.url);
                        // The sync/async boundary: this thread has no ambient
                        // runtime, so the handed-down handle is what enters the
                        // runtime that owns the agent.
                        peer.card = fetch_card(&card_client, &runtime_handle, &url);

                        let _ = cb_tx
                            .try_send(PeerEvent::PeerDiscovered(Box::new(peer)))
                            .inspect_err(|_| {
                                log::warn!("peer event channel full, dropping PeerDiscovered")
                            });
                    }
                    ServiceEvent::ServiceRemoved(_service_type, fullname) => {
                        let name = peer_name_from_fullname(&fullname);
                        let _ = cb_tx.try_send(PeerEvent::PeerLost(name)).inspect_err(|_| {
                            log::warn!("peer event channel full, dropping PeerLost")
                        });
                    }
                    _ => {
                        // ServiceFound, SearchStarted, SearchStopped — ignore.
                    }
                }
            }
            log::debug!(
                "mDNS browse thread stopped: browse replaced by a rediscover or daemon shut down"
            );
        });

        Ok((
            Self {
                poll_thread: Some(handle),
                _daemon: Some(daemon),
            },
            rx,
        ))
    }
}

impl Drop for DiscoveryBrowser {
    fn drop(&mut self) {
        // A `ServiceDaemon` is a cloneable channel handle with no `Drop` impl:
        // dropping this clone disconnects nothing. The browse channel closes
        // when the querier goes away — `stop_browse` removes it and purges its
        // pending retransmissions (the rediscover path), and daemon shutdown
        // does the same. Join the thread once it has exited.
        drop(self._daemon.take());

        if let Some(handle) = self.poll_thread.take() {
            let deadline = std::time::Instant::now() + Duration::from_secs(1);
            while std::time::Instant::now() < deadline {
                if handle.is_finished() {
                    let _ = handle.join();
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            log::warn!("DiscoveryBrowser poll thread did not exit within 1s");
        }
    }
}

// region:    --- Support

/// Best-effort card fetch from the browse thread.
///
/// The browse loop is a bare `std::thread` with no ambient runtime, so
/// [`Handle::block_on`] is the sync/async boundary into the runtime that owns
/// the agent. The deadline is stamped per request: the handed-down transport is
/// the composition root's and stays unbounded for every other caller. Returns
/// `None` on any failure — a peer without a card is still a peer.
fn fetch_card<C: A2aHttpClient>(
    client: &C,
    runtime_handle: &Handle,
    url: &str,
) -> Option<AgentCard> {
    match runtime_handle.block_on(client.get_bytes_with_timeout(url, CARD_FETCH_TIMEOUT)) {
        Ok(bytes) => match serde_json::from_slice::<AgentCard>(&bytes) {
            Ok(card) => Some(card),
            Err(e) => {
                log::warn!("card fetch failed (bad JSON) for {url}: {e}");
                None
            }
        },
        Err(e @ (A2aError::ConnectionRefused(_) | A2aError::Timeout(_))) => {
            log::warn!("card fetch failed (connection error) for {url}: {e}");
            None
        }
        Err(e) => {
            log::warn!("card fetch failed (HTTP error) for {url}: {e}");
            None
        }
    }
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "../../test/discovery/browser.rs"]
mod browser_test;

// endregion: --- Tests
