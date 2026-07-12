use super::*;

// ---------------------------------------------------------------------------
// PeerCache — unit tests
// ---------------------------------------------------------------------------

#[test]
fn test_add_and_list() {
    let cache = PeerCache::new();
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
fn test_add_and_get() {
    let cache = PeerCache::new();
    cache.add_or_update(Peer {
        name: "alice".into(),
        url: "http://127.0.0.1:8080".into(),
        host: "127.0.0.1".into(),
        port: 8080,
        card: None,
        discovered_at: std::time::Instant::now(),
    });
    let peer = cache.get("alice").unwrap();
    assert_eq!(peer.name, "alice");
    assert_eq!(peer.url, "http://127.0.0.1:8080");
}

#[test]
fn test_add_and_get_card() {
    let cache = PeerCache::new();
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
    assert_eq!(cache.card("alice").unwrap().name, "alice");
}

#[test]
fn test_add_then_remove() {
    let cache = PeerCache::new();
    cache.add_or_update(Peer {
        name: "alice".into(),
        ..Default::default()
    });
    cache.remove("alice");
    assert!(cache.get("alice").is_none());
    assert!(cache.list().is_empty());
}

#[test]
fn test_add_update() {
    let cache = PeerCache::new();
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
    assert_eq!(cache.get("alice").unwrap().port, 9090);
}

#[test]
fn test_get_nonexistent() {
    let cache = PeerCache::new();
    assert!(cache.get("nobody").is_none());
}

#[test]
fn test_card_nonexistent() {
    let cache = PeerCache::new();
    assert!(cache.card("nobody").is_none());
}

#[test]
fn test_concurrent_access() {
    use std::sync::Arc;
    let cache = Arc::new(PeerCache::new());
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
