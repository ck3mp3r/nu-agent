use std::sync::Arc;
use std::time::Duration;

use mdns_sd::ServiceDaemon;
use tokio::sync::mpsc;

use crate::TEST_BLOCKING_TIMEOUT;

// Import discovery module items — some are pub(crate) and not re-exported
// via the pub glob in lib.rs.
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

/// Await a [`PeerEvent::PeerDiscovered`] matching `pred` within `budget`.
///
/// Returns `true` when a matching peer arrives, `false` when the budget
/// expires or the channel closes. Non-matching events are skipped.
async fn wait_for_peer<F>(rx: &mut mpsc::Receiver<PeerEvent>, budget: Duration, pred: F) -> bool
where
    F: Fn(&Peer) -> bool,
{
    let deadline = tokio::time::Instant::now() + budget;
    while let Some(remaining) = deadline.checked_duration_since(tokio::time::Instant::now()) {
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Some(PeerEvent::PeerDiscovered(peer))) if pred(&peer) => return true,
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => return false,
        }
    }
    false
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
    let found = wait_for_peer(&mut rx, Duration::from_secs(5), |p| {
        p.name.contains("roundtrip-agent")
    })
    .await;

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
    let wrong_key = wait_for_peer(&mut rx, Duration::from_secs(5), |p| {
        p.name.contains("mesh-key-b-agent")
    })
    .await;
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

    let (_browser, mut rx) = DiscoveryBrowser::browse(daemon.clone(), "test-mesh", 7777).unwrap();

    // Wait up to 5 seconds — our own service must never appear.
    let found_self = wait_for_peer(&mut rx, Duration::from_secs(5), |p| {
        p.name == "self-test-agent"
    })
    .await;
    assert!(!found_self, "agent should not discover itself");
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

    // Daemon B browses with a DIFFERENT mesh_key "key-b"
    let daemon_b = ServiceDaemon::new().unwrap();
    let (browser, mut rx) = DiscoveryBrowser::browse(daemon_b.clone(), "key-b", 52095).unwrap();

    // Wait up to 5 seconds: should NOT discover cross-daemon-agent (key mismatch)
    let found_wrong = wait_for_peer(&mut rx, Duration::from_secs(5), |p| {
        p.name.contains("cross-daemon-agent")
    })
    .await;

    assert!(
        !found_wrong,
        "Should NOT discover cross-daemon-agent with different mesh key"
    );

    drop(browser);
    let _ = daemon_a.shutdown();
    let _ = daemon_b.shutdown();
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

    crate::discovery::card::ensure_crypto_provider();
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await.unwrap();
    assert!(resp.status().is_success());
    let fetched: AgentCard = resp.json().await.unwrap();
    assert_eq!(fetched.name, "test-agent");
    assert_eq!(fetched.description.as_deref(), Some("A test agent"));

    // Verify 404 on old wrong path
    let bad_url = format!("{}/agent.json", server.local_url);
    let resp = client.get(&bad_url).send().await.unwrap();
    assert_eq!(resp.status().as_u16(), 404);

    server.shutdown().await;
}
