use std::time::Duration;

use mdns_sd::ServiceDaemon;
use tokio::sync::mpsc;

use crate::AgentCard;
use crate::discovery::{
    DiscoveryBrowser, DiscoveryService, PeerEvent, SERVICE_TYPE, build_service_info,
};

/// Concrete mDNS-based peer discovery.
///
/// Owns the shared [`ServiceDaemon`] root, the [`DiscoveryService`] for
/// advertising this agent, and the [`DiscoveryBrowser`] for discovering
/// peers.  All three share the same daemon so that registration and browsing
/// go through a single mDNS responder.
#[derive(Default)]
pub struct MdnsPeerDiscovery {
    daemon: Option<ServiceDaemon>,
    service: Option<DiscoveryService>,
    browser: Option<DiscoveryBrowser>,
    peer_rx: Option<mpsc::Receiver<PeerEvent>>,
    /// The full mDNS service name (e.g. `researcher-12345._nu-agent-a2a._tcp.local.`)
    /// set during [`start`](Self::start) and updated by [`rename`](Self::rename).
    fullname: Option<String>,
    /// The instance name (without `._nu-agent-a2a._tcp.local.` suffix).
    /// Stored at `start()` time for use by `reregister()`.
    instance_name: Option<String>,
    /// The port the agent is listening on.
    port: Option<u16>,
    /// The mesh key for discovery isolation.
    mesh_key: Option<String>,
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

        // Capture the full mDNS service name for later unregistration.
        // The fullname is: <instance>.<service_type>.<domain>
        if service.is_some() {
            self.fullname = Some(format!("{agent_name}._nu-agent-a2a._tcp.local."));
            self.instance_name = Some(agent_name.to_string());
            self.port = Some(port);
            self.mesh_key = Some(mesh_key.to_string());
        }

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

    /// The full mDNS service name (e.g. `researcher-12345._nu-agent-a2a._tcp.local.`).
    ///
    /// Returns `None` if mDNS was never started or registration failed.
    pub fn fullname(&self) -> Option<&str> {
        self.fullname.as_deref()
    }

    /// Re-register the mDNS service under a new name.
    ///
    /// Unregisters the old service (triggering `ServiceRemoved` on peers) and
    /// registers a new one with the updated name, port, and card.  The browse
    /// thread continues running — it does not need to be restarted.
    ///
    /// This is a no-op if mDNS was never started or the daemon is gone.
    pub fn rename(
        &mut self,
        old_fullname: &str,
        new_name: &str,
        port: u16,
        card: &AgentCard,
        mesh_key: &str,
    ) {
        let daemon = match self.daemon.as_ref() {
            Some(d) => d,
            None => {
                log::debug!("MdnsPeerDiscovery::rename: no daemon, skipping");
                return;
            }
        };

        // ── Unregister the old service ────────────────────────────────────
        // Use recv_timeout so we don't block indefinitely.  The daemon's
        // flume channel should respond quickly; 500ms is generous.
        match daemon.unregister(old_fullname) {
            Ok(receiver) => {
                if receiver.recv_timeout(Duration::from_millis(500)).is_err() {
                    log::warn!(
                        "mDNS unregister timed out for '{old_fullname}' — continuing anyway"
                    );
                } else {
                    log::info!("mDNS unregistered old service '{old_fullname}'");
                }
            }
            Err(e) => {
                log::warn!("mDNS unregister failed for '{old_fullname}': {e}");
            }
        }

        // ── Register the new service ───────────────────────────────────────
        let info = match build_service_info(new_name, port, card, mesh_key) {
            Ok(i) => i,
            Err(e) => {
                log::warn!("mDNS rename: failed to build ServiceInfo: {e}");
                return;
            }
        };

        match daemon.register(info) {
            Ok(_) => {
                log::info!("mDNS service re-registered as '{new_name}' on port {port}");
                self.fullname = Some(format!("{new_name}._nu-agent-a2a._tcp.local."));
                self.instance_name = Some(new_name.to_string());
                self.port = Some(port);
                self.mesh_key = Some(mesh_key.to_string());
            }
            Err(e) => {
                log::warn!("mDNS rename: registration failed for '{new_name}': {e}");
            }
        }
    }

    /// Start a fresh mDNS browse, replacing the current one.
    ///
    /// The current browse is unsubscribed with `stop_browse` first: a plain
    /// re-browse replaces only the querier map entry while the retransmission
    /// queue keeps a sender clone for the same listener, so the old channel
    /// would stay connected and its thread would park forever. `stop_browse`
    /// drops those senders, so the previous channel closes, the previous
    /// browse thread exits, and dropping the previous [`DiscoveryBrowser`]
    /// joins it.
    ///
    /// Returns the receiver of the new browse, or `None` when mDNS is not
    /// active (no daemon, mesh key, or port).
    pub fn rediscover(&mut self) -> Option<mpsc::Receiver<PeerEvent>> {
        let daemon = match self.daemon.as_ref() {
            Some(d) => d.clone(),
            None => {
                log::debug!("MdnsPeerDiscovery::rediscover: no daemon, skipping");
                return None;
            }
        };
        let mesh_key = match &self.mesh_key {
            Some(k) => k.clone(),
            None => {
                log::debug!("MdnsPeerDiscovery::rediscover: no mesh key, skipping");
                return None;
            }
        };
        let port = match self.port {
            Some(p) => p,
            None => {
                log::debug!("MdnsPeerDiscovery::rediscover: no port, skipping");
                return None;
            }
        };

        // stop_browse is the canonical unsubscribe: it drops the querier sender
        // and purges every pending retransmission, which otherwise keeps a
        // sender clone alive forever. It must run BEFORE the fresh browse —
        // both share the same service-type key.
        if let Err(e) = daemon.stop_browse(SERVICE_TYPE) {
            log::warn!("mDNS rediscover: stop_browse failed for '{SERVICE_TYPE}': {e}");
        }

        match DiscoveryBrowser::browse(daemon, &mesh_key, port) {
            Ok((browser, rx)) => {
                // Replacing the browser drops the previous one, which joins the
                // superseded browse thread before this call returns.
                self.browser = Some(browser);
                log::debug!("mDNS browse re-queried for mesh '{mesh_key}'");
                Some(rx)
            }
            Err(e) => {
                log::warn!("mDNS rediscover: browse failed: {e}");
                None
            }
        }
    }

    /// Re-announce the mDNS service with the current name.
    ///
    /// Rebuilds `ServiceInfo` from the stored instance name, port, mesh key,
    /// and the provided (possibly updated) card, then calls `daemon.register()`
    /// again.  The `mdns-sd` docs say: "To re-announce a service with an updated
    /// service_info, just call `register` again. No need to call `unregister` first."
    ///
    /// This is a no-op if mDNS was never started or the daemon is gone.
    pub fn reregister(&mut self, card: &AgentCard) {
        let daemon = match self.daemon.as_ref() {
            Some(d) => d,
            None => {
                log::debug!("MdnsPeerDiscovery::reregister: no daemon, skipping");
                return;
            }
        };
        let instance_name = match &self.instance_name {
            Some(n) => n,
            None => {
                log::debug!("MdnsPeerDiscovery::reregister: no instance name, skipping");
                return;
            }
        };
        let port = match self.port {
            Some(p) => p,
            None => {
                log::debug!("MdnsPeerDiscovery::reregister: no port, skipping");
                return;
            }
        };
        let mesh_key = match &self.mesh_key {
            Some(k) => k,
            None => {
                log::debug!("MdnsPeerDiscovery::reregister: no mesh key, skipping");
                return;
            }
        };

        let info = match build_service_info(instance_name, port, card, mesh_key) {
            Ok(i) => i,
            Err(e) => {
                log::warn!("mDNS reregister: failed to build ServiceInfo: {e}");
                return;
            }
        };

        match daemon.register(info) {
            Ok(_) => {
                log::debug!("mDNS service re-announced as '{instance_name}' on port {port}");
            }
            Err(e) => {
                log::warn!("mDNS reregister: registration failed for '{instance_name}': {e}");
            }
        }
    }
}
