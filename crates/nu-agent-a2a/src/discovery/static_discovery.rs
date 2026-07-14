use tokio::sync::mpsc;

use crate::Peer;
use crate::discovery::PeerEvent;

/// Concrete static peer discovery.
///
/// Emits all configured peers as [`PeerEvent::PeerDiscovered`] events when
/// [`take_peer_rx`](Self::take_peer_rx) is first called.  Does not advertise
/// or listen — peers are provided ahead of time (e.g., from a config file or
/// Kubernetes API).
pub struct StaticPeerDiscovery {
    peer_rx: Option<mpsc::Receiver<PeerEvent>>,
}

impl StaticPeerDiscovery {
    /// Create a new static discovery instance with the given peers.
    ///
    /// The channel is eagerly populated with all configured peers; callers
    /// can drain it via [`take_peer_rx`](Self::take_peer_rx).
    pub fn new(peers: Vec<Peer>) -> Self {
        let (tx, rx) = mpsc::channel::<PeerEvent>(64);
        for peer in &peers {
            let _ = tx.try_send(PeerEvent::PeerDiscovered(Box::new(peer.clone())));
        }
        Self { peer_rx: Some(rx) }
    }

    /// No-op — static discovery does not advertise.
    pub fn start(
        &mut self,
        _agent_name: &str,
        _port: u16,
        _card: &crate::AgentCard,
        _mesh_key: &str,
    ) {
    }

    /// Take the peer event receiver.
    ///
    /// Returns the receiver populated at construction time on first call.
    /// Subsequent calls return `None`.
    pub fn take_peer_rx(&mut self) -> Option<mpsc::Receiver<PeerEvent>> {
        self.peer_rx.take()
    }

    /// No-op — nothing to shut down.
    pub fn shutdown(&mut self) {}
}
