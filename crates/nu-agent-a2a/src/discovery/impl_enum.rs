use tokio::sync::mpsc;

use crate::AgentCard;
use crate::discovery::PeerEvent;
use crate::discovery::mdns_discovery::MdnsPeerDiscovery;
use crate::discovery::static_discovery::StaticPeerDiscovery;

/// Concrete peer discovery implementations.
///
/// Enum dispatch — no dynamic dispatch.  Add variants here when adding new
/// discovery backends (e.g., Kubernetes API, file-based, etc.).
pub enum PeerDiscoveryImpl {
    /// mDNS-based discovery (advertise + browse).
    Mdns(Box<MdnsPeerDiscovery>),
    /// Static list of pre-configured peers.
    Static(StaticPeerDiscovery),
    /// No-op — no discovery.
    Noop,
}

impl PeerDiscoveryImpl {
    /// Start advertising and browsing.
    pub fn start(&mut self, agent_name: &str, port: u16, card: &AgentCard, mesh_key: &str) {
        match self {
            PeerDiscoveryImpl::Mdns(m) => m.start(agent_name, port, card, mesh_key),
            PeerDiscoveryImpl::Static(_) => {} // static config doesn't advertise
            PeerDiscoveryImpl::Noop => {}
        }
    }

    /// Get the peer discovery channel.
    pub fn take_peer_rx(&mut self) -> Option<mpsc::Receiver<PeerEvent>> {
        match self {
            PeerDiscoveryImpl::Mdns(m) => m.take_peer_rx(),
            PeerDiscoveryImpl::Static(s) => s.take_peer_rx(),
            PeerDiscoveryImpl::Noop => None,
        }
    }

    /// Stop discovery (send goodbye, etc.).
    pub fn shutdown(&mut self) {
        match self {
            PeerDiscoveryImpl::Mdns(m) => m.shutdown(),
            PeerDiscoveryImpl::Static(_) => {}
            PeerDiscoveryImpl::Noop => {}
        }
    }

    /// The full mDNS service name, if mDNS is active.
    pub fn fullname(&self) -> Option<&str> {
        match self {
            PeerDiscoveryImpl::Mdns(m) => m.fullname(),
            _ => None,
        }
    }

    /// Re-register the mDNS service under a new name.
    ///
    /// For non-mDNS variants this is a no-op (logged at debug level).
    pub fn rename(
        &mut self,
        old_fullname: &str,
        new_name: &str,
        port: u16,
        card: &AgentCard,
        mesh_key: &str,
    ) {
        match self {
            PeerDiscoveryImpl::Mdns(m) => m.rename(old_fullname, new_name, port, card, mesh_key),
            PeerDiscoveryImpl::Static(_) => {
                log::debug!("PeerDiscoveryImpl::rename: static discovery, no-op");
            }
            PeerDiscoveryImpl::Noop => {
                log::debug!("PeerDiscoveryImpl::rename: noop discovery, no-op");
            }
        }
    }

    /// Re-announce the mDNS service with the current name and an updated card.
    ///
    /// For non-mDNS variants this is a no-op (logged at debug level).
    pub fn reregister(&mut self, card: &AgentCard) {
        match self {
            PeerDiscoveryImpl::Mdns(m) => m.reregister(card),
            PeerDiscoveryImpl::Static(_) => {
                log::debug!("PeerDiscoveryImpl::reregister: static discovery, no-op");
            }
            PeerDiscoveryImpl::Noop => {
                log::debug!("PeerDiscoveryImpl::reregister: noop discovery, no-op");
            }
        }
    }
}
