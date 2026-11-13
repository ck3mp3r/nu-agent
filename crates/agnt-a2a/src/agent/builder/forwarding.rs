//! Peer-event forwarding: what an arriving peer event does to the cache.
//!
//! The startup browse receiver and every rediscover receiver feed the same two
//! functions, so the semantics a discovery lands with live here and nowhere else.

use std::sync::Arc;

use tokio::sync::mpsc;

use crate::discovery::PeerEvent;
use crate::peer::PeerCache;

/// Apply one peer event to the cache, logging the transition.
///
/// Pure cache logic — no network and no await — so the startup forwarding task
/// and every rediscover forwarding task share exactly these semantics.
///
/// A discovery is news only when the cache entry is new or its URL or card name
/// moved. Every rediscover re-announces the peers it finds, and that peer is
/// already cached, so the repeat is a refresh: same facts, one debug line.
pub(crate) fn apply_peer_event(cache: &PeerCache, own_url: &str, event: &PeerEvent) {
    match event {
        PeerEvent::PeerDiscovered(peer) => {
            if is_peer_change(cache, peer) {
                log::info!("mesh peer discovered: {} ({})", peer.name, peer.url);
            } else {
                log::debug!("mesh peer refreshed: {} ({})", peer.name, peer.url);
            }
            cache.add_or_update(peer.as_ref().clone());
        }
        PeerEvent::PeerLost(name) => {
            log::warn!("mesh peer lost: {name}");
            cache.remove(name);
            let other_peers = cache
                .list()
                .iter()
                .filter(|peer| peer.url != own_url)
                .count();
            if other_peers == 0 {
                log::warn!("mesh peer cache empty — no peers besides this agent");
            }
        }
    }
}

/// Whether `peer` brings the cache something it does not hold.
///
/// The cache is keyed by the mDNS instance name, so the same name is the same
/// entry: a discovery changes it only when its URL or its card's name moved.
fn is_peer_change(cache: &PeerCache, peer: &crate::peer::Peer) -> bool {
    let Some(cached) = cache.get(&peer.name) else {
        return true;
    };
    cached.url != peer.url
        || cached.card.as_ref().map(|card| &card.name) != peer.card.as_ref().map(|card| &card.name)
}

/// Forward every event from `rx` into the cache until the channel closes.
///
/// Used for the startup browse receiver and for every receiver handed back by
/// a rediscover; the superseded task ends when its channel disconnects.
pub(crate) async fn forward_peer_events(
    mut rx: mpsc::Receiver<PeerEvent>,
    cache: Arc<PeerCache>,
    own_url: String,
) {
    while let Some(event) = rx.recv().await {
        apply_peer_event(&cache, &own_url, &event);
    }
    log::debug!(
        "peer event forwarding stopped: browse receiver closed by a rediscover or shutdown"
    );
}

// region:    --- Tests

#[cfg(test)]
#[path = "../../../test/agent/forwarding.rs"]
mod forwarding_test;

// endregion: --- Tests
