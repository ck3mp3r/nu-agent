use std::sync::Arc;
use std::time::Instant;

use super::{apply_peer_event, forward_peer_events};
use crate::discovery::PeerEvent;
use crate::{Peer, PeerCache};
use tokio::sync::mpsc;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Test Support

const OWN_URL: &str = "http://127.0.0.1:40000";

fn peer_fixture(name: &str, port: u16) -> Peer {
    Peer {
        name: name.to_string(),
        url: format!("http://127.0.0.1:{port}"),
        host: "127.0.0.1".to_string(),
        port,
        card: None,
        discovered_at: Instant::now(),
    }
}

// endregion: --- Test Support

// region:    --- Tests

#[tokio::test]
async fn test_forward_peer_events_adds_discovered_peer() -> Result<()> {
    // -- Setup & Fixtures
    let cache = Arc::new(PeerCache::default());
    cache.add_or_update(peer_fixture("agent-40000", 40000));
    let (tx, rx) = mpsc::channel(8);

    // -- Exec
    tx.send(PeerEvent::PeerDiscovered(Box::new(peer_fixture(
        "researcher-51234",
        51234,
    ))))
    .await
    .map_err(|e| format!("send should succeed: {e}"))?;
    drop(tx);
    forward_peer_events(rx, cache.clone(), OWN_URL.to_string()).await;

    // -- Check
    let cached = cache
        .get("researcher-51234")
        .ok_or("discovered peer should land in the cache")?;
    assert_eq!(cached.url, "http://127.0.0.1:51234");
    assert_eq!(cache.list().len(), 2);
    Ok(())
}

#[tokio::test]
async fn test_forward_peer_events_removes_peer_on_lost() -> Result<()> {
    // -- Setup & Fixtures
    let cache = Arc::new(PeerCache::default());
    cache.add_or_update(peer_fixture("agent-40000", 40000));
    cache.add_or_update(peer_fixture("researcher-51234", 51234));
    let (tx, rx) = mpsc::channel(8);

    // -- Exec
    tx.send(PeerEvent::PeerLost("researcher-51234".to_string()))
        .await
        .map_err(|e| format!("send should succeed: {e}"))?;
    drop(tx);
    forward_peer_events(rx, cache.clone(), OWN_URL.to_string()).await;

    // -- Check
    assert!(cache.get("researcher-51234").is_none());
    let own = cache
        .get("agent-40000")
        .ok_or("own entry must survive a peer loss")?;
    assert_eq!(own.url, OWN_URL);
    Ok(())
}

#[tokio::test]
async fn test_apply_peer_event_loss_of_unknown_peer_keeps_own_entry() -> Result<()> {
    // -- Setup & Fixtures
    let cache = PeerCache::default();
    cache.add_or_update(peer_fixture("agent-40000", 40000));

    // -- Exec
    apply_peer_event(
        &cache,
        OWN_URL,
        &PeerEvent::PeerLost("ghost-51999".to_string()),
    );

    // -- Check
    assert_eq!(cache.list().len(), 1);
    assert!(cache.get("agent-40000").is_some());
    Ok(())
}

// endregion: --- Tests
