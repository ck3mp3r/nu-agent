use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// PeerCache — unit tests
// ---------------------------------------------------------------------------

#[test]
fn test_add_and_list() {
    let cache = PeerCache::default();
    cache.add_or_update(Peer {
        name: "a".into(),
        url: "http://127.0.0.1:1".into(),
        host: "127.0.0.1".into(),
        port: 1,
        card: None,
        discovered_at: std::time::Instant::now(),
    });
    cache.add_or_update(Peer {
        name: "b".into(),
        url: "http://127.0.0.1:2".into(),
        host: "127.0.0.1".into(),
        port: 2,
        card: None,
        discovered_at: std::time::Instant::now(),
    });
    cache.add_or_update(Peer {
        name: "c".into(),
        url: "http://127.0.0.1:3".into(),
        host: "127.0.0.1".into(),
        port: 3,
        card: None,
        discovered_at: std::time::Instant::now(),
    });
    assert_eq!(cache.list().len(), 3);
}

#[test]
fn test_add_and_get() -> Result<()> {
    let cache = PeerCache::default();
    cache.add_or_update(Peer {
        name: "alice".into(),
        url: "http://127.0.0.1:8080".into(),
        host: "127.0.0.1".into(),
        port: 8080,
        card: None,
        discovered_at: std::time::Instant::now(),
    });
    let peer = cache.get("alice").ok_or("should find alice peer")?;
    assert_eq!(peer.name, "alice");
    assert_eq!(peer.url, "http://127.0.0.1:8080");
    Ok(())
}

#[test]
fn test_add_and_get_card() -> Result<()> {
    let cache = PeerCache::default();
    let card = AgentCard {
        name: "alice".into(),
        ..Default::default()
    };
    cache.add_or_update(Peer {
        name: "alice".into(),
        url: "http://127.0.0.1:8080".into(),
        host: "127.0.0.1".into(),
        port: 8080,
        card: Some(card.clone()),
        discovered_at: std::time::Instant::now(),
    });
    let card = cache.card("alice").ok_or("should find alice card")?;
    assert_eq!(card.name, "alice");
    Ok(())
}

#[test]
fn test_add_then_remove() {
    let cache = PeerCache::default();
    cache.add_or_update(Peer {
        name: "alice".into(),
        ..Default::default()
    });
    cache.remove("alice");
    assert!(cache.get("alice").is_none());
    assert!(cache.list().is_empty());
}

#[test]
fn test_add_update() -> Result<()> {
    let cache = PeerCache::default();
    cache.add_or_update(Peer {
        name: "alice".into(),
        url: "http://127.0.0.1:8080".into(),
        host: "127.0.0.1".into(),
        port: 8080,
        card: None,
        discovered_at: std::time::Instant::now(),
    });
    cache.add_or_update(Peer {
        name: "alice".into(),
        url: "http://127.0.0.1:9090".into(),
        host: "127.0.0.1".into(),
        port: 9090,
        card: None,
        discovered_at: std::time::Instant::now(),
    });
    let peer = cache.get("alice").ok_or("should find alice peer")?;
    assert_eq!(peer.port, 9090);
    Ok(())
}

#[test]
fn test_get_nonexistent() {
    let cache = PeerCache::default();
    assert!(cache.get("nobody").is_none());
}

#[test]
fn test_card_nonexistent() {
    let cache = PeerCache::default();
    assert!(cache.card("nobody").is_none());
}

#[test]
fn test_concurrent_access() {
    use std::sync::Arc;
    let cache = Arc::new(PeerCache::default());
    let mut handles = vec![];
    for i in 0..10 {
        let c = cache.clone();
        handles.push(std::thread::spawn(move || {
            c.add_or_update(Peer {
                name: format!("agent-{i}"),
                ..Default::default()
            });
            c.get(&format!("agent-{i}"));
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(cache.list().len(), 10);
}

#[test]
fn peer_cache_updates_on_name_change() {
    let cache = PeerCache::default();
    cache.add_or_update(Peer {
        name: "researcher-12345".into(),
        url: "http://127.0.0.1:9999".into(),
        host: "127.0.0.1".into(),
        port: 9999,
        card: None,
        discovered_at: std::time::Instant::now(),
    });
    assert!(cache.get("researcher-12345").is_some());

    // Simulate agent switch: remove old name, add new name
    cache.remove("researcher-12345");
    cache.add_or_update(Peer {
        name: "reviewer".into(),
        url: "http://127.0.0.1:9999".into(),
        host: "127.0.0.1".into(),
        port: 9999,
        card: None,
        discovered_at: std::time::Instant::now(),
    });

    let peers = cache.list();
    let names: Vec<&str> = peers.iter().map(|p| p.name.as_str()).collect();
    assert!(
        names.contains(&"reviewer"),
        "list should contain the new name 'reviewer', got: {names:?}"
    );
    assert!(
        !names.contains(&"researcher-12345"),
        "list should NOT contain the old name 'researcher-12345', got: {names:?}"
    );
}
