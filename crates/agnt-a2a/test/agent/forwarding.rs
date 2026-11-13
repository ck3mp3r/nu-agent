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

/// Every record captured on this thread since the test set up, cleared as it is
/// read.
///
/// `testing_logger` captures into a thread-local buffer, so two tests never see
/// each other's records: under `cargo test`, which runs the whole suite in one
/// process on a shared thread pool, and under nextest's process-per-test alike.
fn captured() -> Vec<(log::Level, String)> {
    let found = std::cell::RefCell::new(Vec::new());
    testing_logger::validate(|logs| {
        *found.borrow_mut() = logs
            .iter()
            .map(|log| (log.level, log.body.clone()))
            .collect();
    });
    found.into_inner()
}

/// The records of `logs` whose message contains `fragment`.
fn matching(logs: &[(log::Level, String)], fragment: &str) -> Vec<(log::Level, String)> {
    logs.iter()
        .filter(|(_, body)| body.contains(fragment))
        .cloned()
        .collect()
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

/// WHEN a rediscover re-announces a peer the cache already holds, THEN the
/// discovery SHALL log at debug, because nothing about the peer changed.
#[test]
fn test_apply_peer_event_logs_a_repeat_at_debug_and_a_change_at_info() -> Result<()> {
    // -- Setup & Fixtures
    testing_logger::setup();
    let cache = PeerCache::default();
    let mut peer = peer_fixture("researcher-51234", 51234);
    apply_peer_event(
        &cache,
        OWN_URL,
        &PeerEvent::PeerDiscovered(Box::new(peer.clone())),
    );

    // -- Exec: the same peer again — a rediscover's re-announcement — then a
    // peer whose card name moved.
    apply_peer_event(
        &cache,
        OWN_URL,
        &PeerEvent::PeerDiscovered(Box::new(peer.clone())),
    );
    peer.card = Some(crate::AgentCard {
        name: "renamed".to_string(),
        ..Default::default()
    });
    apply_peer_event(&cache, OWN_URL, &PeerEvent::PeerDiscovered(Box::new(peer)));

    // -- Check
    let logs = captured();
    let found = matching(&logs, "mesh peer");
    assert_eq!(found.len(), 3, "one line per discovery: {found:?}");
    assert_eq!(
        found[0].0,
        log::Level::Info,
        "the first sighting is news: {found:?}"
    );
    assert_eq!(
        found[1].0,
        log::Level::Debug,
        "the rediscover repeat is not news: {found:?}"
    );
    assert_eq!(
        found[2].0,
        log::Level::Info,
        "a changed card name is news again: {found:?}"
    );
    Ok(())
}

// endregion: --- Tests
