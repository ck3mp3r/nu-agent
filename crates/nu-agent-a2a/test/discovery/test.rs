use std::sync::Arc;
use std::time::Duration;

use mdns_sd::ServiceDaemon;
use tokio::sync::mpsc;

use crate::TEST_BLOCKING_TIMEOUT;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// Import discovery module items — some are pub(crate) and not re-exported
// via the pub glob in lib.rs.
use crate::discovery::static_discovery::StaticPeerDiscovery;
use crate::discovery::{DiscoveryBrowser, DiscoveryService, PeerEvent};
use crate::{A2aServer, AgentCard, Peer, PeerCache};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_card(name: &str, port: u16) -> AgentCard {
    AgentCard {
        name: name.to_string(),
        url: format!("http://127.0.0.1:{port}"),
        skills: vec![],
        ..Default::default()
    }
}

/// Convert raw resolved-service fields into a [`Peer`].
///
/// This mirrors the mapping inside [`peer_from_service`] so we can unit-test
/// the conversion logic without constructing an `mdns_sd::ResolvedService`
/// (which has no public constructor).
fn peer_from_fields(name: &str, address: &str, port: u16, hostname: &str) -> Peer {
    Peer {
        name: name.to_string(),
        url: format!("http://{address}:{port}"),
        host: hostname.to_string(),
        port,
        card: None,
        discovered_at: std::time::Instant::now(),
    }
}

// ---------------------------------------------------------------------------
// PeerEvent dispatch tests
// ---------------------------------------------------------------------------

/// Verify that sending a [`PeerEvent::PeerDiscovered`] through the channel
/// preserves event order.
#[tokio::test]
async fn test_peer_event_roundtrip_discovered() -> Result<()> {
    let (tx, mut rx) = mpsc::channel::<PeerEvent>(16);

    let peer = Box::new(peer_from_fields("alpha", "10.0.0.1", 9001, "alpha.local."));
    tx.send(PeerEvent::PeerDiscovered(peer)).await.unwrap();
    drop(tx);

    let event = rx
        .recv()
        .await
        .ok_or("should receive PeerEvent after send")?;
    match event {
        PeerEvent::PeerDiscovered(p) => {
            assert_eq!(p.name, "alpha");
            assert_eq!(p.port, 9001);
        }
        other => panic!("Expected PeerDiscovered, got: {other:?}"),
    }
    Ok(())
}

/// Verify that sending multiple discovered events in sequence works.
#[tokio::test]
async fn test_peer_event_roundtrip_multiple() {
    let (tx, mut rx) = mpsc::channel::<PeerEvent>(16);

    let alpha = Box::new(peer_from_fields("alpha", "10.0.0.1", 9001, "alpha.local."));
    let beta = Box::new(peer_from_fields("beta", "10.0.0.2", 9002, "beta.local."));
    tx.send(PeerEvent::PeerDiscovered(alpha)).await.unwrap();
    tx.send(PeerEvent::PeerDiscovered(beta)).await.unwrap();
    drop(tx);

    let mut names = Vec::new();
    while let Some(event) = rx.recv().await {
        if let PeerEvent::PeerDiscovered(p) = event {
            names.push(p.name);
        }
    }

    assert!(names.iter().any(|n| n == "alpha"));
    assert!(names.iter().any(|n| n == "beta"));
}

/// Verify that sending a [`PeerEvent::PeerLost`] is received correctly.
#[tokio::test]
async fn test_peer_event_roundtrip_lost() {
    let (tx, mut rx) = mpsc::channel::<PeerEvent>(16);

    let peer = Box::new(peer_from_fields(
        "removable",
        "10.0.0.1",
        9001,
        "removable.local.",
    ));
    tx.send(PeerEvent::PeerDiscovered(peer)).await.unwrap();
    tx.send(PeerEvent::PeerLost("removable".to_string()))
        .await
        .unwrap();
    drop(tx);

    let mut discovered = false;
    let mut lost = false;

    while let Some(event) = rx.recv().await {
        match event {
            PeerEvent::PeerDiscovered(p) => {
                if p.name == "removable" {
                    discovered = true;
                }
            }
            PeerEvent::PeerLost(name) => {
                if name == "removable" {
                    lost = true;
                }
            }
        }
    }

    assert!(discovered, "Should have received PeerDiscovered");
    assert!(lost, "Should have received PeerLost");
}

// ---------------------------------------------------------------------------
// DiscoveryService — registration (smoke tests)
// ---------------------------------------------------------------------------

/// Verify that we can create a service, register it, and drop it without
/// errors.
///
/// NOTE: This test requires a working mDNS responder (mdns-sd internal) on
/// the host. It is ignored by default in CI/offline environments.
#[ignore]
#[tokio::test]
async fn test_register_service() {
    let card = test_card("test-agent", 9999);
    let daemon = ServiceDaemon::new().unwrap();
    let service =
        DiscoveryService::register(daemon, "test-agent", 9999, &card, "test-mesh").unwrap();
    drop(service);
}

/// Verify that registering the same name twice does not error.
///
/// NOTE: Requires a working mDNS responder on the host.
#[ignore]
#[tokio::test]
async fn test_register_duplicate_name() {
    let daemon = ServiceDaemon::new().unwrap();
    let card = test_card("dup-agent", 3001);
    let a =
        DiscoveryService::register(daemon.clone(), "dup-agent", 3001, &card, "test-mesh").unwrap();
    let b =
        DiscoveryService::register(daemon.clone(), "dup-agent", 3002, &card, "test-mesh").unwrap();
    drop(a);
    drop(b);
    let _ = daemon.shutdown();
}

// ---------------------------------------------------------------------------
// DiscoveryBrowser — smoke tests
// ---------------------------------------------------------------------------

/// Integration test: register a service, browse for it, verify discovery.
///
/// NOTE: Requires a working mDNS responder (loopback) on the host.
#[ignore]
#[tokio::test]
async fn test_register_and_browse_roundtrip() {
    let daemon = ServiceDaemon::new().unwrap();
    let card = test_card("roundtrip-agent", 7777);
    let _service =
        DiscoveryService::register(daemon.clone(), "roundtrip-agent", 7777, &card, "test-mesh")
            .unwrap();

    // Use a different port so we still discover the registered service
    // (self-port filter only excludes matching port+localhost).
    let (_browser, mut rx) = DiscoveryBrowser::browse(daemon.clone(), "test-mesh", 52095).unwrap();

    // Wait up to 5 seconds for the registered service to appear.
    let mut found = false;
    for _ in 0..50 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        while let Ok(event) = rx.try_recv() {
            if let PeerEvent::PeerDiscovered(peer) = event
                && peer.name.contains("roundtrip-agent")
            {
                found = true;
            }
        }

        if found {
            break;
        }
    }

    assert!(found, "Should discover registered service via mDNS");
}

// ---------------------------------------------------------------------------
// Mesh scoping integration tests
// ---------------------------------------------------------------------------

/// Verify that browsing with one mesh key does not discover services
/// registered with a different mesh key.
///
/// NOTE: Requires a working mDNS responder on the host.
#[tokio::test]
async fn test_mesh_scoping_filters_other_key() {
    let daemon = ServiceDaemon::new().unwrap();
    let card_a = test_card("mesh-key-a-agent", 7778);
    let card_b = test_card("mesh-key-b-agent", 7779);
    let _service_a =
        DiscoveryService::register(daemon.clone(), "mesh-key-a-agent", 7778, &card_a, "key-a")
            .unwrap();
    let _service_b =
        DiscoveryService::register(daemon.clone(), "mesh-key-b-agent", 7779, &card_b, "key-b")
            .unwrap();

    let (_browser, mut rx) = DiscoveryBrowser::browse(daemon.clone(), "key-a", 52095).unwrap();

    // Wait up to 5 seconds and check we never receive the wrong-key peer.
    let mut wrong_key = false;
    for _ in 0..50 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        while let Ok(event) = rx.try_recv() {
            if let PeerEvent::PeerDiscovered(peer) = event
                && peer.name.contains("mesh-key-b-agent")
            {
                wrong_key = true;
            }
        }
        if wrong_key {
            break;
        }
    }
    assert!(
        !wrong_key,
        "Should not discover peers with different mesh key"
    );
}

// ---------------------------------------------------------------------------
// Self-exclusion: agent should not discover itself
// ---------------------------------------------------------------------------

/// Verify that an agent does not discover its own mDNS service.
///
/// NOTE: Flaky due to mDNS resolution timing — 127.0.0.1 must be
/// present in the resolved addresses for the SelfPort filter to match.
/// Requires a working mDNS responder on the host.
#[tokio::test]
#[ignore]
async fn test_browse_self_excluded() {
    let daemon = ServiceDaemon::new().unwrap();
    let card = test_card("self-test-agent", 7777);
    let _service =
        DiscoveryService::register(daemon.clone(), "self-test-agent", 7777, &card, "test-mesh")
            .unwrap();

    // Give mDNS time to announce
    tokio::time::sleep(Duration::from_millis(500)).await;

    let (_browser, mut rx) = DiscoveryBrowser::browse(daemon.clone(), "test-mesh", 7777).unwrap();

    // Try to receive — should NOT get our own service
    tokio::time::sleep(Duration::from_secs(1)).await;
    while let Ok(event) = rx.try_recv() {
        match event {
            PeerEvent::PeerDiscovered(p) => {
                assert_ne!(
                    p.name, "self-test-agent",
                    "agent should not discover itself"
                );
            }
            PeerEvent::PeerLost(_) => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Cross-daemon mesh scoping (two separate daemons)
// ---------------------------------------------------------------------------

/// Verify that browsing with one mesh key does NOT discover services
/// registered with a DIFFERENT mesh key on a SEPARATE daemon.
///
/// NOTE: Requires a working mDNS responder on the host.
#[tokio::test]
async fn test_cross_daemon_mesh_scoping() {
    // Daemon A registers a service with mesh_key "key-a"
    let daemon_a = ServiceDaemon::new().unwrap();
    let card_a = test_card("cross-daemon-agent", 8888);
    let _service_a = DiscoveryService::register(
        daemon_a.clone(),
        "cross-daemon-agent",
        8888,
        &card_a,
        "key-a",
    )
    .unwrap();

    // Give mDNS time to announce
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Daemon B browses with a DIFFERENT mesh_key "key-b"
    let daemon_b = ServiceDaemon::new().unwrap();
    let (browser, mut rx) = DiscoveryBrowser::browse(daemon_b.clone(), "key-b", 52095).unwrap();

    // Wait and check: should NOT discover cross-daemon-agent (key mismatch)
    let mut found_wrong = false;
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_millis(200)).await;
        while let Ok(event) = rx.try_recv() {
            if let PeerEvent::PeerDiscovered(peer) = event
                && peer.name.contains("cross-daemon-agent")
            {
                found_wrong = true;
                eprintln!("FOUND peer despite key mismatch! peer_key from TXT?");
            }
        }
    }

    assert!(
        !found_wrong,
        "Should NOT discover cross-daemon-agent with different mesh key"
    );

    drop(browser);
    let _ = daemon_a.shutdown();
    let _ = daemon_b.shutdown();
}

// ---------------------------------------------------------------------------
// StaticPeerDiscovery
// ---------------------------------------------------------------------------

/// Verify that [`StaticPeerDiscovery`] emits all configured peers through
/// the channel on first call to [`take_peer_rx`].
#[tokio::test]
async fn test_static_discovery_emits_configured_peers() -> Result<()> {
    let peers = vec![
        Peer {
            name: "alpha".into(),
            url: "http://10.0.0.1:9001".into(),
            host: "alpha.local.".into(),
            port: 9001,
            ..Default::default()
        },
        Peer {
            name: "beta".into(),
            url: "http://10.0.0.2:9002".into(),
            host: "beta.local.".into(),
            port: 9002,
            ..Default::default()
        },
    ];

    let mut discovery = StaticPeerDiscovery::new(peers);
    let mut rx = discovery.take_peer_rx().ok_or("should have peer_rx")?;

    // Collect all events non-blockingly.
    let mut names: Vec<String> = Vec::new();
    while let Ok(event) = rx.try_recv() {
        if let PeerEvent::PeerDiscovered(peer) = event {
            names.push(peer.name.clone());
        }
    }

    assert_eq!(names.len(), 2, "should emit two peers");
    assert!(names.contains(&"alpha".to_string()));
    assert!(names.contains(&"beta".to_string()));

    // Second call returns None.
    assert!(
        discovery.take_peer_rx().is_none(),
        "second take should be None"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Agent card fetch integration test
// ---------------------------------------------------------------------------

/// Start a real A2A server, fetch its agent card at the correct URL
/// (`/.well-known/agent-card.json`), and verify the old wrong path
/// (`/agent.json`) returns 404.
///
/// NOTE: Requires a working network stack (loopback). Ignored by default
/// in CI.
#[tokio::test]
#[ignore]
async fn test_card_fetch_roundtrip() {
    let card = AgentCard {
        name: "test-agent".into(),
        description: Some("A test agent".into()),
        ..Default::default()
    };

    let cache = Arc::new(PeerCache::default());
    let server = A2aServer::start_with_blocking_timeout(card, cache, 0, TEST_BLOCKING_TIMEOUT)
        .await
        .unwrap();
    let url = format!("{}/.well-known/agent-card.json", server.local_url);

    let resp = reqwest::get(&url).await.unwrap();
    assert!(resp.status().is_success());
    let fetched: AgentCard = resp.json().await.unwrap();
    assert_eq!(fetched.name, "test-agent");
    assert_eq!(fetched.description.as_deref(), Some("A test agent"));

    // Verify 404 on old wrong path
    let bad_url = format!("{}/agent.json", server.local_url);
    let resp = reqwest::get(&bad_url).await.unwrap();
    assert_eq!(resp.status().as_u16(), 404);

    server.shutdown().await;
}
