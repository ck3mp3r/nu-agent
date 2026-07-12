use std::collections::HashMap;
use std::sync::RwLock;
use std::time::Instant;

use crate::AgentCard;

/// A remote peer discovered via A2A mDNS.
#[derive(Clone)]
pub struct Peer {
    pub name: String,
    pub url: String,
    pub host: String,
    pub port: u16,
    pub card: Option<AgentCard>,
    pub discovered_at: Instant,
}

impl Default for Peer {
    fn default() -> Self {
        Self {
            name: String::new(),
            url: String::new(),
            host: String::new(),
            port: 0,
            card: None,
            discovered_at: Instant::now(),
        }
    }
}

impl std::fmt::Debug for Peer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Peer")
            .field("name", &self.name)
            .field("url", &self.url)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("card", &self.card.as_ref().map(|c| &c.name))
            .field(
                "discovered_at_secs_ago",
                &self.discovered_at.elapsed().as_secs(),
            )
            .finish()
    }
}

/// A thread-safe cache of discovered peers, keyed by peer name.
pub struct PeerCache {
    peers: RwLock<HashMap<String, Peer>>,
}

impl Clone for PeerCache {
    fn clone(&self) -> Self {
        let peers = self.peers.read().expect("PeerCache read lock poisoned");
        Self {
            peers: RwLock::new(peers.clone()),
        }
    }
}

impl Default for PeerCache {
    fn default() -> Self {
        Self::new()
    }
}

impl PeerCache {
    pub fn new() -> Self {
        Self {
            peers: RwLock::new(HashMap::new()),
        }
    }

    pub fn add_or_update(&self, peer: Peer) {
        let mut peers = self.peers.write().expect("PeerCache write lock poisoned");
        peers.insert(peer.name.clone(), peer);
    }

    pub fn remove(&self, name: &str) {
        let mut peers = self.peers.write().expect("PeerCache write lock poisoned");
        peers.remove(name);
    }

    pub fn get(&self, name: &str) -> Option<Peer> {
        let peers = self.peers.read().expect("PeerCache read lock poisoned");
        peers.get(name).cloned()
    }

    pub fn list(&self) -> Vec<Peer> {
        let peers = self.peers.read().expect("PeerCache read lock poisoned");
        peers.values().cloned().collect()
    }

    pub fn card(&self, name: &str) -> Option<AgentCard> {
        let peers = self.peers.read().expect("PeerCache read lock poisoned");
        peers.get(name).and_then(|p| p.card.clone())
    }
}
