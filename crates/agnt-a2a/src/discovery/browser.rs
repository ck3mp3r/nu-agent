use std::thread::JoinHandle;
use std::time::Duration;

use mdns_sd::{ServiceDaemon, ServiceEvent};
use tokio::sync::mpsc;

use crate::{A2aError, peer::Peer};

use super::card::{CARD_FETCHER, CardFetcher};
use super::filter;
use super::service::{peer_from_service, peer_name_from_fullname};

// ---------------------------------------------------------------------------
// DiscoveryBrowser — mDNS service browsing
// ---------------------------------------------------------------------------

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
    // NOTE: field order matters for Drop.  When the manual Drop impl runs,
    // it drops the daemon first (disconnecting the flume channel), then
    // joins the poll thread.  The daemon field is *last* so that if Rust's
    // auto-drop runs after the manual Drop, the daemon outlives the thread.
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
    pub fn browse(
        daemon: ServiceDaemon,
        mesh_key: &str,
        own_port: u16,
    ) -> Result<(Self, mpsc::Receiver<PeerEvent>), A2aError> {
        let receiver = daemon
            .browse("_nu-agent-a2a._tcp.local.")
            .map_err(|e| A2aError::Internal(format!("mdns-sd browse: {e}")))?;

        let (tx, rx) = mpsc::channel::<PeerEvent>(64);
        let cb_tx = tx.clone();
        let mesh_key = mesh_key.to_string();
        let filters = filter::build_filters(own_port, &mesh_key);

        let handle = std::thread::spawn(move || {
            // Initialise card fetcher inside the browse thread — safe because
            // we are *not* inside a tokio runtime here.
            if CARD_FETCHER.set(CardFetcher::new()).is_err() {
                log::warn!("CardFetcher already initialized — duplicate browse thread");
            }

            // recv() blocks until an event arrives.  When the daemon is
            // shut down (on drop) the channel disconnects and recv()
            // returns Err.
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
                        peer.card = CARD_FETCHER.get().and_then(|f| f.fetch_card(&url));

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
            log::error!("mDNS browse thread: daemon channel closed — peer discovery stopped");
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
        // Drop the daemon first (disconnects the flume channel in the browse
        // thread), then join the thread.
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
