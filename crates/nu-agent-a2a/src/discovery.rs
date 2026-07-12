use std::sync::Arc;
use std::sync::Once;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use reqwest::Client;
use tokio::sync::mpsc;
use zeroconf::browser::{BrowserEvent, ServiceDiscovery};
use zeroconf::prelude::*;
use zeroconf::{MdnsBrowser, MdnsService, ServiceType, TxtRecord};

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
/// poll thread (a `std::thread`, not a tokio context) so that dropping the
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

/// Build a [`TxtRecord`] from agent metadata for mDNS advertisement.
fn build_txt_properties(agent_name: &str, port: u16, card: &AgentCard) -> TxtRecord {
    let mut txt = TxtRecord::new();
    let _ = txt.insert("name", agent_name);
    let _ = txt.insert("version", env!("CARGO_PKG_VERSION"));
    let _ = txt.insert("url", &format!("http://127.0.0.1:{port}"));

    if let Some(desc) = &card.description {
        let truncated: String = desc.chars().take(255).collect();
        let _ = txt.insert("description", &truncated);
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
        let _ = txt.insert("skills", &skill_ids);
    }
    txt
}

// ---------------------------------------------------------------------------
// DiscoveryService — mDNS service registration
// ---------------------------------------------------------------------------

/// Registers this agent as an mDNS service on `_nu-agent-a2a._tcp.local.`.
///
/// The [`EventLoop`](zeroconf::EventLoop) returned by
/// [`MdnsService::register()`](zeroconf::service::TMdnsService::register)
/// references internal Bonjour/Avahi state owned by the [`MdnsService`]
/// handle.  Both must live together and the handle must outlive the event
/// loop.  To enforce this the handle lives on this struct rather than inside
/// the polling thread, and Rust's field-drop order guarantees it is dropped
/// *after* the thread has been joined.
pub struct DiscoveryService {
    /// Bonjour/Avahi service handle — dropped *after* the polling thread has
    /// exited (see field declaration order and the manual [`Drop`] impl).
    _service: MdnsService,
    stop_flag: Arc<AtomicBool>,
    poll_thread: Option<JoinHandle<()>>,
}

impl DiscoveryService {
    /// Register a new A2A agent as an mDNS service.
    ///
    /// The service is automatically deregistered when the returned handle is
    /// dropped (see [`Drop`]).
    pub fn register(agent_name: &str, port: u16, card: &AgentCard) -> Result<Self, A2aError> {
        let service_type = ServiceType::new("nu-agent-a2a", "tcp")
            .map_err(|e| A2aError::Internal(format!("service type: {e}")))?;

        let mut service = MdnsService::new(service_type, port);
        service.set_name(agent_name);

        let txt = build_txt_properties(agent_name, port, card);
        service.set_txt_record(txt);

        let event_loop = service
            .register()
            .map_err(|e| A2aError::Internal(format!("zeroconf register: {e}")))?;

        let stop_flag = Arc::new(AtomicBool::new(false));
        let flag = stop_flag.clone();

        let handle = std::thread::spawn(move || {
            loop {
                if flag.load(Ordering::Relaxed) {
                    break;
                }
                if let Err(e) = event_loop.poll(Duration::from_millis(100)) {
                    log::error!("zeroconf poll error: {e}");
                }
            }
        });

        Ok(Self {
            _service: service,
            stop_flag,
            poll_thread: Some(handle),
        })
    }

    /// Create a no-op instance (no mDNS registration).
    ///
    /// Use when registration fails and the caller wants to continue running
    /// without mDNS discoverability.
    pub fn noop() -> Self {
        let dummy_type = ServiceType::new("dummy", "tcp")
            .unwrap_or_else(|_| ServiceType::new("_dummy", "_tcp").ok().unwrap());
        Self {
            _service: MdnsService::new(dummy_type, 0),
            stop_flag: Arc::new(AtomicBool::new(false)),
            poll_thread: None,
        }
    }
}

impl Drop for DiscoveryService {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Some(handle) = self.poll_thread.take() {
            // The 100ms poll interval means the thread sees stop_flag and
            // exits within ~100ms.  Bound the join as a safety net.
            let deadline = std::time::Instant::now() + Duration::from_secs(1);
            while std::time::Instant::now() < deadline {
                if handle.is_finished() {
                    let _ = handle.join();
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            log::warn!("DiscoveryService poll thread did not exit within 1s");
        }
        // `service` drops here (field declaration order), *after* the polling
        // thread has exited and the EventLoop inside it has been dropped.
    }
}

/// Convert a zeroconf [`ServiceDiscovery`] into a [`Peer`].
///
/// This is the shared factory used by both production callbacks and tests
/// to ensure consistent [`Peer`] construction.
pub(crate) fn peer_from_service(service: &ServiceDiscovery) -> Peer {
    Peer {
        name: service.name().clone(),
        url: format!("http://{}:{}", service.address(), service.port()),
        host: service.host_name().clone(),
        port: *service.port(),
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
/// The [`MdnsBrowser`] handle is owned by this struct to keep the underlying
/// Bonjour/Avahi browser alive for the [`EventLoop`](zeroconf::EventLoop) in
/// the polling thread (same reasoning as [`DiscoveryService`]).
pub struct DiscoveryBrowser {
    /// Bonjour/Avahi browser handle — dropped *after* the polling thread has
    /// exited (see field declaration order and the manual [`Drop`] impl).
    _browser: MdnsBrowser,
    stop_flag: Arc<AtomicBool>,
    poll_thread: Option<JoinHandle<()>>,
}

impl DiscoveryBrowser {
    /// Start browsing for A2A agents.
    ///
    /// Returns a handle that can be dropped to stop browsing, and a
    /// `mpsc::Receiver` that yields [`PeerEvent`]s as peers are discovered
    /// or lost.
    pub fn browse() -> Result<(Self, mpsc::Receiver<PeerEvent>), A2aError> {
        let service_type = ServiceType::new("nu-agent-a2a", "tcp")
            .map_err(|e| A2aError::Internal(format!("service type: {e}")))?;

        let mut browser = MdnsBrowser::new(service_type);
        let (tx, rx) = mpsc::channel::<PeerEvent>(64);
        let cb_tx = tx.clone();

        browser.set_service_callback(Box::new(move |result, _ctx| {
            let event = match result {
                Ok(e) => e,
                Err(e) => {
                    log::error!("zeroconf browse error: {e}");
                    return;
                }
            };

            let maybe_card = match &event {
                BrowserEvent::Add(service) => {
                    let url = format!("http://{}:{}/agent.json", service.address(), service.port());
                    CARD_FETCHER.get().and_then(|f| f.fetch_card(&url))
                }
                _ => None,
            };

            match &event {
                BrowserEvent::Add(service) => {
                    let mut peer = peer_from_service(service);
                    peer.card = maybe_card;
                    let _ = cb_tx
                        .try_send(PeerEvent::PeerDiscovered(Box::new(peer)))
                        .inspect_err(|_| {
                            log::warn!("peer event channel full, dropping PeerDiscovered")
                        });
                }
                BrowserEvent::Remove(info) => {
                    let _ = cb_tx
                        .try_send(PeerEvent::PeerLost(info.name().clone()))
                        .inspect_err(|_| log::warn!("peer event channel full, dropping PeerLost"));
                }
            }
        }));

        let event_loop = browser
            .browse_services()
            .map_err(|e| A2aError::Internal(format!("zeroconf browse: {e}")))?;

        let stop_flag = Arc::new(AtomicBool::new(false));
        let flag = stop_flag.clone();

        let handle = std::thread::spawn(move || {
            // Initialise card fetcher inside the poll thread — safe because
            // we are *not* inside a tokio runtime here.
            let _ = CARD_FETCHER.set(CardFetcher::new());

            loop {
                if flag.load(Ordering::Relaxed) {
                    break;
                }
                if let Err(e) = event_loop.poll(Duration::from_millis(100)) {
                    log::error!("zeroconf browse poll error: {e}");
                }
            }
        });

        Ok((
            Self {
                _browser: browser,
                stop_flag,
                poll_thread: Some(handle),
            },
            rx,
        ))
    }
}

impl Drop for DiscoveryBrowser {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
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
