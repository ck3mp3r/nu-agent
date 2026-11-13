use std::collections::HashMap;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

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

/// How many consecutive card-fetch failures for one URL are warnings.
///
/// A dead peer re-resolves on every scan and every rediscover. The first
/// failures name the URL and its transport error; every failure after that
/// repeats a fact the log already holds.
const FETCH_MAX_WARNS: u32 = 2;

/// How long one URL's card is left alone after it keeps failing.
///
/// The cooldown turns a per-scan retry into one attempt per
/// `PEER_FETCH_COOLDOWN`, which is what keeps a dead port out of the log.
const PEER_FETCH_COOLDOWN: Duration = Duration::from_secs(300);

/// The per-URL fetch and announcement state the browse thread keeps.
///
/// Thread-local by design: the browse thread owns the map, so nothing locks it.
/// A rediscover starts a fresh browse — and a fresh map — so the re-announcement
/// that follows is left to the consumer, whose cache diff is what keeps it
/// quiet.
#[derive(Default)]
struct FetchState {
    /// The mDNS instance name this URL was resolved as, so `PeerLost` can clear
    /// the entry under the name the removal carries.
    name: String,
    /// Consecutive card-fetch failures, reset by a success.
    consecutive_failures: u32,
    /// Set after the warnings are spent: the URL's card is not fetched again
    /// until this instant.
    cooldown_until: Option<Instant>,
    /// Whether this URL's card is already in hand.
    ///
    /// A held card retires the fetch, not the peer: the sweep refreshes every
    /// cached card every `SWEEP_INTERVAL`, so the browse thread asks once and a
    /// rediscover stays cheap. A URL whose fetch never succeeded holds no card,
    /// which is what keeps its retries — and their warnings — bounded.
    card_held: bool,
    /// Whether this URL's `PeerDiscovered` was published already.
    ///
    /// The querier re-resolves every live peer on its own schedule, and the
    /// cache the event feeds is identity-keyed and idempotent, so a repeat has
    /// nothing to add until `PeerLost` forgets the URL.
    announced: bool,
}

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
            // The per-URL fetch and announcement state, owned by this thread for
            // as long as this browse lives.
            let mut states: HashMap<String, FetchState> = HashMap::new();
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

                        let peer = peer_from_service(&resolved);
                        let Some(peer) = resolve_peer(
                            &mut states,
                            peer,
                            &card_client,
                            &runtime_handle,
                            Instant::now(),
                        ) else {
                            continue;
                        };

                        let _ = cb_tx
                            .try_send(PeerEvent::PeerDiscovered(Box::new(peer)))
                            .inspect_err(|_| {
                                log::warn!("peer event channel full, dropping PeerDiscovered")
                            });
                    }
                    ServiceEvent::ServiceRemoved(_service_type, fullname) => {
                        let name = peer_name_from_fullname(&fullname);
                        // A lost peer's URL is forgotten, so a peer that comes
                        // back announces itself again instead of staying silent
                        // as a repeat.
                        states.retain(|_, state| state.name != name);
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

/// Resolve one service into the peer the browse thread publishes.
///
/// `None` when this resolution is a repeat: the URL was announced already, and
/// the cache the event feeds is identity-keyed, so publishing again would only
/// re-log a fact nothing changed. The mDNS querier re-resolves every live peer
/// on its own schedule, so this dedupe is what keeps a scan out of the log.
///
/// `states` is the browse thread's per-URL map. A card-fetch failure counts
/// against its URL: the first [`FETCH_MAX_WARNS`] failures are warnings, and
/// every failure after that is a repeat — logged at debug — which also arms the
/// [`PEER_FETCH_COOLDOWN`] that keeps a dead port out of the log. A success
/// clears the count and logs the recovery once.
fn resolve_peer<C: A2aHttpClient>(
    states: &mut HashMap<String, FetchState>,
    peer: Peer,
    client: &C,
    runtime_handle: &Handle,
    now: Instant,
) -> Option<Peer> {
    let url = format!("{}/.well-known/agent-card.json", peer.url);
    let mut peer = peer;
    let state = states.entry(peer.url.clone()).or_default();
    state.name = peer.name.clone();

    match (state.card_held, state.cooldown_until) {
        // Inside the cooldown the card is not asked for: the peer keeps the
        // identity it has, and the dead port costs one attempt per cooldown.
        (false, Some(until)) if until > now => {
            log::debug!(
                "card fetch skipped for {url}: {}s of cooldown left",
                (until - now).as_secs()
            );
        }
        // The card is in hand: the sweep refreshes held cards, not this thread.
        (true, _) => {}
        (false, _) => match fetch_card(client, runtime_handle, &url) {
            Ok(card) => {
                if state.consecutive_failures > 0 {
                    log::info!(
                        "card fetch recovered for {url} after {} failed attempts",
                        state.consecutive_failures
                    );
                }
                state.consecutive_failures = 0;
                state.cooldown_until = None;
                state.card_held = true;
                peer.card = Some(card);
            }
            Err(reason) => {
                state.consecutive_failures += 1;
                let failures = state.consecutive_failures;
                if failures <= FETCH_MAX_WARNS {
                    log::warn!("card fetch failed for {url}: {reason}");
                } else {
                    log::debug!(
                        "card fetch failed again ({failures} consecutive) for {url}: {reason}"
                    );
                    state.cooldown_until = Some(now + PEER_FETCH_COOLDOWN);
                }
            }
        },
    }

    if state.announced {
        log::debug!("peer re-resolved: {} ({url})", peer.name);
        return None;
    }
    state.announced = true;
    log::info!("peer resolved: {} ({url})", peer.name);
    Some(peer)
}

/// Best-effort card fetch from the browse thread.
///
/// The browse loop is a bare `std::thread` with no ambient runtime, so
/// [`Handle::block_on`] is the sync/async boundary into the runtime that owns
/// the agent. The deadline is stamped per request: the handed-down transport is
/// the composition root's and stays unbounded for every other caller. A failure
/// answers the reason only — the caller decides what level it is worth, because
/// it is the caller that knows how often this URL has failed.
fn fetch_card<C: A2aHttpClient>(
    client: &C,
    runtime_handle: &Handle,
    url: &str,
) -> Result<AgentCard, String> {
    let bytes = runtime_handle
        .block_on(client.get_bytes_with_timeout(url, CARD_FETCH_TIMEOUT))
        .map_err(|e| match e {
            A2aError::ConnectionRefused(_) | A2aError::Timeout(_) => {
                format!("connection error: {e}")
            }
            other => format!("HTTP error: {other}"),
        })?;
    serde_json::from_slice::<AgentCard>(&bytes).map_err(|e| format!("bad JSON: {e}"))
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "../../test/discovery/browser.rs"]
mod browser_test;

// endregion: --- Tests
