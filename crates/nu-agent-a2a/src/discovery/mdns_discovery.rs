use mdns_sd::ServiceDaemon;
use tokio::sync::mpsc;

use crate::AgentCard;
use crate::discovery::{DiscoveryBrowser, DiscoveryService, PeerEvent};

/// Concrete mDNS-based peer discovery.
///
/// Owns the shared [`ServiceDaemon`] root, the [`DiscoveryService`] for
/// advertising this agent, and the [`DiscoveryBrowser`] for discovering
/// peers.  All three share the same daemon so that registration and browsing
/// go through a single mDNS responder.
pub struct MdnsPeerDiscovery {
    daemon: Option<ServiceDaemon>,
    service: Option<DiscoveryService>,
    browser: Option<DiscoveryBrowser>,
    peer_rx: Option<mpsc::Receiver<PeerEvent>>,
}

impl MdnsPeerDiscovery {
    /// Create an unstarted mDNS discovery instance.
    ///
    /// Call [`start`](Self::start) to begin advertising and browsing.
    pub fn new() -> Self {
        Self {
            daemon: None,
            service: None,
            browser: None,
            peer_rx: None,
        }
    }
}

impl Default for MdnsPeerDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

impl MdnsPeerDiscovery {
    /// Start mDNS advertising and browsing.
    ///
    /// Creates a shared [`ServiceDaemon`], registers this agent as an mDNS
    /// service, and starts browsing for other agents on the same mesh.
    ///
    /// All errors are non-fatal — they are logged and the instance continues
    /// in a reduced-capability mode (no mDNS at all if the daemon fails,
    /// only advertising if browsing fails, etc.).
    pub fn start(&mut self, agent_name: &str, port: u16, card: &AgentCard, mesh_key: &str) {
        // ── Shared mDNS daemon ────────────────────────────────────────────
        // One daemon for both register and browse so they share the same
        // mDNS responder state.  Without this, macOS's mDNSResponder
        // absorbs browse queries without forwarding them to the register
        // daemon's thread.
        let daemon = match ServiceDaemon::new() {
            Ok(d) => d,
            Err(e) => {
                log::warn!("mDNS daemon creation failed (discovery disabled): {e}");
                return;
            }
        };

        // ── mDNS service registration ─────────────────────────────────────
        let service =
            match DiscoveryService::register(daemon.clone(), agent_name, port, card, mesh_key) {
                Ok(s) => {
                    log::info!("mDNS service registered as '{agent_name}' on port {port}");
                    Some(s)
                }
                Err(e) => {
                    log::warn!("mDNS registration failed (non-fatal): {e}");
                    None
                }
            };

        // ── mDNS discovery browser ────────────────────────────────────────
        let (browser, peer_rx) = match DiscoveryBrowser::browse(daemon.clone(), mesh_key, port) {
            Ok((b, rx)) => {
                log::info!("mDNS browsing started for mesh '{mesh_key}'");
                (Some(b), Some(rx))
            }
            Err(e) => {
                log::warn!("mDNS browsing failed (non-fatal): {e}");
                (None, None)
            }
        };

        self.daemon = Some(daemon);
        self.service = service;
        self.browser = browser;
        self.peer_rx = peer_rx;
    }

    /// Take the peer event receiver, if browsing was started.
    ///
    /// Returns `None` if browsing failed or has already been taken.
    pub fn take_peer_rx(&mut self) -> Option<mpsc::Receiver<PeerEvent>> {
        self.peer_rx.take()
    }

    /// Shut down the mDNS daemon, stopping both registration and browsing.
    pub fn shutdown(&mut self) {
        if let Some(d) = self.daemon.take() {
            let _ = d.shutdown();
        }
    }
}
