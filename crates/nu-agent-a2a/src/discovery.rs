use std::sync::Once;
use std::thread::JoinHandle;
use std::time::Duration;

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use reqwest::Client;
use tokio::sync::mpsc;

use crate::{A2aError, AgentCard, peer::Peer};

/// Ensure the rustls crypto provider is installed before creating a reqwest
/// [`Client`] that uses `rustls-no-provider`.  Safe to call multiple times.
fn ensure_crypto_provider() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// Shared state for best-effort card fetching, populated lazily inside the
/// browse thread (a `std::thread`, not a tokio context) so that dropping the
/// inner `tokio::runtime::Runtime` never triggers the "Cannot drop a runtime
/// in a blocking context" panic.
struct CardFetcher {
    client: Option<Client>,
    rt: Option<tokio::runtime::Runtime>,
}

impl CardFetcher {
    fn new() -> Self {
        ensure_crypto_provider();
        let client = Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .ok();
        let rt = tokio::runtime::Runtime::new().ok();
        Self { client, rt }
    }

    /// Best-effort fetch of an [`AgentCard`] from the given URL.
    /// Returns `None` on any error (connection refused, timeout, non-200,
    /// bad JSON).
    fn fetch_card(&self, url: &str) -> Option<AgentCard> {
        let (client, rt) = (self.client.as_ref()?, self.rt.as_ref()?);
        rt.block_on(async {
            let resp = client.get(url).send().await.ok()?;
            if !resp.status().is_success() {
                return None;
            }
            resp.json::<AgentCard>().await.ok()
        })
    }
}

static CARD_FETCHER: std::sync::OnceLock<CardFetcher> = std::sync::OnceLock::new();

/// Build TXT properties as key-value pairs for mDNS advertisement.
fn build_txt_properties(agent_name: &str, port: u16, card: &AgentCard, mesh_key: &str) -> Vec<(String, String)> {
    let mut properties = Vec::new();
    properties.push(("name".to_string(), agent_name.to_string()));
    properties.push(("version".to_string(), env!("CARGO_PKG_VERSION").to_string()));
    properties.push(("url".to_string(), format!("http://127.0.0.1:{port}")));
    properties.push(("mesh_key".to_string(), mesh_key.to_string()));

    if let Some(desc) = &card.description {
        let truncated: String = desc.chars().take(255).collect();
        properties.push(("description".to_string(), truncated));
    }
    if !card.skills.is_empty() {
        let skill_ids: String = card
            .skills
            .iter()
            .map(|s| s.id.as_str())
            .collect::<Vec<_>>()
            .join(",")
            .chars()
            .take(255)
            .collect();
        properties.push(("skills".to_string(), skill_ids));
    }
    properties
}

/// Extract the instance name from a full mDNS service name.
///
/// The fullname has the form `<instance>.<service_type>.<domain>`.  We split
/// at the first unescaped `.` and return the instance part.  This mirrors
/// `zeroconf`'s `ServiceDiscovery::name()` which returned just the instance.
fn peer_name_from_fullname(fullname: &str) -> String {
    // Simple split at first '.' — agent names rarely contain dots.  An
    // escaped dot (`\.`) in the instance portion is not handled here, but
    // mdns-sd handles it during registration.
    fullname.split('.').next().unwrap_or(fullname).to_string()
}

/// Extract the address portion of an mdns-sd [`ScopedIp`].
fn format_scoped_ip(addr: &mdns_sd::ScopedIp) -> String {
    // ScopedIp implements Display, producing either an IPv4 or IPv6 string
    // (with IPv6 scope suffix when applicable).
    addr.to_string()
}

// ---------------------------------------------------------------------------
// DiscoveryService — mDNS service registration
// ---------------------------------------------------------------------------

/// Registers this agent as an mDNS service on `_nu-agent-a2a._tcp.local.`.
///
/// Uses the pure-Rust [`ServiceDaemon`] from the `mdns-sd` crate — no C FFI
/// dependency (fixes SIGBUS on aarch64 Nix builds).
pub struct DiscoveryService {
    _daemon: Option<ServiceDaemon>,
}

impl DiscoveryService {
    /// Register a new A2A agent as an mDNS service, using a shared daemon.
    ///
    /// The daemon is shared with the [`DiscoveryBrowser`] so that both
    /// registration and browsing go through a single mDNS responder.
    pub fn register(
        daemon: ServiceDaemon,
        agent_name: &str,
        port: u16,
        card: &AgentCard,
        mesh_key: &str,
    ) -> Result<Self, A2aError> {
        let properties = build_txt_properties(agent_name, port, card, mesh_key);

        // Convert Vec<(String, String)> to a slice of (&str, &str) refs.
        let prop_refs: Vec<(&str, &str)> = properties
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();

        // The hostname is mandatory for the mDNS SRV record (RFC 6763)
        // but is never resolved — enable_addr_auto populates A records
        // with all host IPs (127.0.0.1 + external interfaces).
        let hostname = "a2a.local."; // static dummy, DNS-safe, no agent name dependency

        let info = ServiceInfo::new(
            "_nu-agent-a2a._tcp.local.",
            agent_name,
            hostname,
            "", // ip — empty at creation, daemon fills in via addr_auto
            port,
            // Pass the slice explicitly — `&[(K, V)]` implements
            // `IntoTxtProperties` when K, V: ToString.
            &prop_refs[..],
        )
        .map_err(|e| A2aError::Internal(format!("mdns-sd service info: {e}")))?
        .enable_addr_auto(); // daemon populates host IPs from all interfaces

        daemon
            .register(info)
            .map_err(|e| A2aError::Internal(format!("mdns-sd register: {e}")))?;

        Ok(Self {
            _daemon: Some(daemon),
        })
    }

    /// Create a no-op instance (no mDNS registration).
    ///
    /// Use when registration fails and the caller wants to continue running
    /// without mDNS discoverability.
    pub fn noop() -> Self {
        Self { _daemon: None }
    }
}

/// Convert an mdns-sd [`ResolvedService`](mdns_sd::ResolvedService) into a [`Peer`].
///
/// This extracts the fields and produces a [`Peer`] with no agent card (the
/// card is fetched separately in the browse loop).
pub(crate) fn peer_from_service(service: &mdns_sd::ResolvedService) -> Peer {
    let name = peer_name_from_fullname(service.get_fullname());
    // Prefer IPv4 over IPv6 — IPv6 link-local scoped addresses
    // (fe80::...) break reqwest HTTP connections.
    let address = service
        .get_addresses()
        .iter()
        .find(|a| a.is_ipv4())
        .or_else(|| service.get_addresses().iter().next())
        .map(format_scoped_ip)
        .unwrap_or_else(|| "0.0.0.0".to_string());

    Peer {
        name,
        url: format!("http://{address}:{}", service.get_port()),
        host: service.get_hostname().to_string(),
        port: service.get_port(),
        card: None,
        discovered_at: std::time::Instant::now(),
    }
}

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
pub struct DiscoveryBrowser {
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
    pub fn browse(daemon: ServiceDaemon, mesh_key: &str, own_name: &str) -> Result<(Self, mpsc::Receiver<PeerEvent>), A2aError> {
        let receiver = daemon
            .browse("_nu-agent-a2a._tcp.local.")
            .map_err(|e| A2aError::Internal(format!("mdns-sd browse: {e}")))?;

        let (tx, rx) = mpsc::channel::<PeerEvent>(64);
        let cb_tx = tx.clone();
        let mesh_key = mesh_key.to_string();
        let own_name = own_name.to_string();

        let handle = std::thread::spawn(move || {
            // Initialise card fetcher inside the browse thread — safe because
            // we are *not* inside a tokio runtime here.
            let _ = CARD_FETCHER.set(CardFetcher::new());

            // recv() blocks until an event arrives.  When the daemon is
            // shut down (on drop) the channel disconnects and recv()
            // returns Err.
            while let Ok(service_event) = receiver.recv() {
                match service_event {
                    ServiceEvent::ServiceResolved(resolved) => {
                        // Skip self — the shared daemon receives its own mDNS responses.
                        let own_fullname = format!("{}._nu-agent-a2a._tcp.local.", own_name);
                        if resolved.get_fullname() == own_fullname {
                            continue;
                        }

                        // Mesh scoping: skip peers with non-matching key
                        let props = resolved.get_properties();
                        let peer_key = props
                            .get("mesh_key")
                            .and_then(|v| v.val())
                            .and_then(|bytes| std::str::from_utf8(bytes).ok());
                        match peer_key {
                            Some(k) if k != mesh_key => continue,
                            Some(_) => {}
                            None => {
                                log::warn!(
                                    "skipping peer {}: no mesh_key in TXT records",
                                    resolved.get_fullname()
                                );
                                continue;
                            }
                        }

                        let address = resolved
                            .get_addresses()
                            .iter()
                            .next()
                            .map(format_scoped_ip)
                            .unwrap_or_else(|| "0.0.0.0".to_string());
                        let url = format!("http://{address}:{}/agent.json", resolved.get_port());
                        let maybe_card = CARD_FETCHER.get().and_then(|f| f.fetch_card(&url));

                        let mut peer = peer_from_service(&resolved);
                        peer.card = maybe_card;

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
