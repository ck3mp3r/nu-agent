use std::sync::Arc;
use std::time::Duration;

use mdns_sd::ServiceDaemon;
use tokio::sync::mpsc;
use tokio::time::timeout;

use crate::TEST_BLOCKING_TIMEOUT;

// Import discovery module items — some are pub(crate) and not re-exported
// via the pub glob in lib.rs.
use crate::discovery::mdns_discovery::MdnsPeerDiscovery;
use crate::discovery::{DiscoveryBrowser, DiscoveryService, PeerDiscoveryImpl, PeerEvent};
use crate::{A2aServer, AgentCard, Peer, PeerCache};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

mod support;
use support::*;

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
async fn test_register_service() -> Result<()> {
    let card = test_card("test-agent", 9999);
    let daemon = ServiceDaemon::new().map_err(|e| format!("{e:?}"))?;
    let service = DiscoveryService::register(daemon, "test-agent", 9999, &card, "test-mesh")
        .map_err(|e| format!("{e:?}"))?;
    drop(service);
    Ok(())
}

/// Verify that registering the same name twice does not error.
///
/// NOTE: Requires a working mDNS responder on the host.
#[ignore]
#[tokio::test]
async fn test_register_duplicate_name() -> Result<()> {
    let daemon = ServiceDaemon::new().map_err(|e| format!("{e:?}"))?;
    let card = test_card("dup-agent", 3001);
    let a = DiscoveryService::register(daemon.clone(), "dup-agent", 3001, &card, "test-mesh")
        .map_err(|e| format!("{e:?}"))?;
    let b = DiscoveryService::register(daemon.clone(), "dup-agent", 3002, &card, "test-mesh")
        .map_err(|e| format!("{e:?}"))?;
    drop(a);
    drop(b);
    let _ = daemon.shutdown();
    Ok(())
}

// ---------------------------------------------------------------------------
// DiscoveryBrowser — smoke tests
// ---------------------------------------------------------------------------

/// Integration test: register a service, browse for it, verify discovery.
///
/// NOTE: Requires a working mDNS responder (loopback) on the host.
#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn test_register_and_browse_roundtrip() -> Result<()> {
    let daemon = ServiceDaemon::new().map_err(|e| format!("{e:?}"))?;
    let card = test_card("roundtrip-agent", 7777);
    let _service =
        DiscoveryService::register(daemon.clone(), "roundtrip-agent", 7777, &card, "test-mesh")
            .map_err(|e| format!("{e:?}"))?;

    // Use a different port so we still discover the registered service
    // (self-port filter only excludes matching port+localhost).
    let (_browser, mut rx) = DiscoveryBrowser::browse(
        daemon.clone(),
        "test-mesh",
        52095,
        test_card_client(),
        tokio::runtime::Handle::current(),
    )
    .map_err(|e| format!("{e:?}"))?;

    // Wait up to 5 seconds for the registered service to appear.
    let found = wait_for_peer(&mut rx, Duration::from_secs(5), |p| {
        p.name.contains("roundtrip-agent")
    })
    .await;

    assert!(found, "Should discover registered service via mDNS");
    Ok(())
}

/// Verify browse returns a live channel.
///
/// NOTE: Requires a working mDNS responder on the host.
#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn test_browse_returns_live_channel() -> Result<()> {
    let daemon = ServiceDaemon::new().map_err(|e| format!("{e:?}"))?;
    let (_browser, rx) = DiscoveryBrowser::browse(
        daemon,
        "test-mesh",
        52095,
        test_card_client(),
        tokio::runtime::Handle::current(),
    )
    .map_err(|e| format!("{e:?}"))?;
    assert!(!rx.is_closed(), "receiver should be alive after browse()");
    Ok(())
}

// ---------------------------------------------------------------------------
// Mesh scoping integration tests
// ---------------------------------------------------------------------------

/// Verify that browsing with one mesh key does not discover services
/// registered with a different mesh key.
///
/// NOTE: Requires a working mDNS responder on the host.
#[tokio::test(flavor = "multi_thread")]
async fn test_mesh_scoping_filters_other_key() -> Result<()> {
    let daemon = ServiceDaemon::new().map_err(|e| format!("{e:?}"))?;
    let card_a = test_card("mesh-key-a-agent", 7778);
    let card_b = test_card("mesh-key-b-agent", 7779);
    let _service_a =
        DiscoveryService::register(daemon.clone(), "mesh-key-a-agent", 7778, &card_a, "key-a")
            .map_err(|e| format!("{e:?}"))?;
    let _service_b =
        DiscoveryService::register(daemon.clone(), "mesh-key-b-agent", 7779, &card_b, "key-b")
            .map_err(|e| format!("{e:?}"))?;

    let (_browser, mut rx) = DiscoveryBrowser::browse(
        daemon.clone(),
        "key-a",
        52095,
        test_card_client(),
        tokio::runtime::Handle::current(),
    )
    .map_err(|e| format!("{e:?}"))?;

    // Wait up to 5 seconds and check we never receive the wrong-key peer.
    let wrong_key = wait_for_peer(&mut rx, Duration::from_secs(5), |p| {
        p.name.contains("mesh-key-b-agent")
    })
    .await;
    assert!(
        !wrong_key,
        "Should not discover peers with different mesh key"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Self-exclusion: agent should not discover itself
// ---------------------------------------------------------------------------

/// Verify that an agent does not discover its own mDNS service.
///
/// NOTE: Flaky due to mDNS resolution timing — 127.0.0.1 must be
/// present in the resolved addresses for the SelfPort filter to match.
/// Requires a working mDNS responder on the host.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_browse_self_excluded() -> Result<()> {
    let daemon = ServiceDaemon::new().map_err(|e| format!("{e:?}"))?;
    let card = test_card("self-test-agent", 7777);
    let _service =
        DiscoveryService::register(daemon.clone(), "self-test-agent", 7777, &card, "test-mesh")
            .map_err(|e| format!("{e:?}"))?;

    let (_browser, mut rx) = DiscoveryBrowser::browse(
        daemon.clone(),
        "test-mesh",
        7777,
        test_card_client(),
        tokio::runtime::Handle::current(),
    )
    .map_err(|e| format!("{e:?}"))?;

    // Wait up to 5 seconds — our own service must never appear.
    let found_self = wait_for_peer(&mut rx, Duration::from_secs(5), |p| {
        p.name == "self-test-agent"
    })
    .await;
    assert!(!found_self, "agent should not discover itself");
    Ok(())
}

// ---------------------------------------------------------------------------
// Cross-daemon mesh scoping (two separate daemons)
// ---------------------------------------------------------------------------

/// Verify that browsing with one mesh key does NOT discover services
/// registered with a DIFFERENT mesh key on a SEPARATE daemon.
///
/// NOTE: Requires a working mDNS responder on the host.
#[tokio::test(flavor = "multi_thread")]
async fn test_cross_daemon_mesh_scoping() -> Result<()> {
    // Daemon A registers a service with mesh_key "key-a"
    let daemon_a = ServiceDaemon::new().map_err(|e| format!("{e:?}"))?;
    let card_a = test_card("cross-daemon-agent", 8888);
    let _service_a = DiscoveryService::register(
        daemon_a.clone(),
        "cross-daemon-agent",
        8888,
        &card_a,
        "key-a",
    )
    .map_err(|e| format!("{e:?}"))?;

    // Daemon B browses with a DIFFERENT mesh_key "key-b"
    let daemon_b = ServiceDaemon::new().map_err(|e| format!("{e:?}"))?;
    let (browser, mut rx) = DiscoveryBrowser::browse(
        daemon_b.clone(),
        "key-b",
        52095,
        test_card_client(),
        tokio::runtime::Handle::current(),
    )
    .map_err(|e| format!("{e:?}"))?;

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
    Ok(())
}

// ---------------------------------------------------------------------------
// Rediscover leak pin
// ---------------------------------------------------------------------------

/// Verify `rediscover()` unsubscribes the previous browse: each superseded
/// peer-event receiver closes within 5 s while the newest one stays open.
///
/// A plain re-browse leaves the previous listener in the retransmission queue
/// holding a sender clone, so the superseded receiver would stay open and the
/// browse thread would park forever. Requires a working mDNS responder.
#[tokio::test(flavor = "multi_thread")]
async fn test_rediscover_closes_superseded_receivers() -> Result<()> {
    // -- Setup & Fixtures
    let card = test_card("rediscover-agent", 52097);
    let mut discovery = PeerDiscoveryImpl::Mdns(Box::new(test_mdns()));
    discovery.start("rediscover-agent-52097", 52097, &card, "rediscover-mesh");
    let mut rx1 = discovery
        .take_peer_rx()
        .ok_or("start should open a browse receiver")?;

    // -- Exec
    let mut rx2 = discovery
        .rediscover()
        .ok_or("first rediscover should return a receiver")?;
    let first_closed = timeout(Duration::from_secs(5), rx1.recv()).await;

    let mut rx3 = discovery
        .rediscover()
        .ok_or("second rediscover should return a receiver")?;
    let second_closed = timeout(Duration::from_secs(5), rx2.recv()).await;

    let third_open = timeout(Duration::from_millis(200), rx3.recv()).await;

    // -- Check
    assert!(
        matches!(first_closed, Ok(None)),
        "rx1 should close within 5s of its supersession, got {first_closed:?}"
    );
    assert!(
        matches!(second_closed, Ok(None)),
        "rx2 should close within 5s of its supersession, got {second_closed:?}"
    );
    assert!(
        third_open.is_err(),
        "rx3 must stay open after its browse starts, got {third_open:?}"
    );

    discovery.shutdown();
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
async fn test_card_fetch_roundtrip() -> Result<()> {
    let card = AgentCard {
        name: "test-agent".into(),
        description: Some("A test agent".into()),
        ..Default::default()
    };

    let cache = Arc::new(PeerCache::default());
    let server = A2aServer::start_with_blocking_timeout(card, cache, 0, TEST_BLOCKING_TIMEOUT)
        .await
        .map_err(|e| format!("{e:?}"))?;
    let url = format!("{}/.well-known/agent-card.json", server.local_url);

    crate::test_support::install_crypto_provider();
    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert!(resp.status().is_success());
    let fetched: AgentCard = resp.json().await.map_err(|e| format!("{e:?}"))?;
    assert_eq!(fetched.name, "test-agent");
    assert_eq!(fetched.description.as_deref(), Some("A test agent"));

    // Verify 404 on old wrong path
    let bad_url = format!("{}/agent.json", server.local_url);
    let resp = client
        .get(&bad_url)
        .send()
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(resp.status().as_u16(), 404);

    server.shutdown().await;
    Ok(())
}

// ---------------------------------------------------------------------------
// MdnsPeerDiscovery — not-started state
// ---------------------------------------------------------------------------
//
// These pin the no-daemon guards. They live in the integration module because a
// `MdnsPeerDiscovery` is constructed with the browse pair its thread uses — a
// live transport — so a unit test that builds one is integration in disguise.

/// A backend over the test transport. It owns no daemon until `start` runs.
fn test_mdns() -> MdnsPeerDiscovery {
    MdnsPeerDiscovery::new(test_card_client(), tokio::runtime::Handle::current())
}

#[tokio::test]
async fn reregister_does_not_panic_when_not_started() {
    let mut discovery = test_mdns();
    discovery.reregister(&test_card("test", 0));
}

#[tokio::test]
async fn reregister_preserves_fullname_when_no_daemon() {
    let mut discovery = test_mdns();
    assert!(discovery.fullname().is_none());
    discovery.reregister(&test_card("test", 0));
    assert!(discovery.fullname().is_none());
}

#[tokio::test]
async fn reregister_with_updated_card_does_not_panic() {
    let mut discovery = test_mdns();
    let updated_card = AgentCard {
        description: Some("updated".to_string()),
        ..test_card("test", 0)
    };
    discovery.reregister(&updated_card);
}

#[tokio::test]
async fn shutdown_then_reregister_does_not_panic() {
    let mut discovery = test_mdns();
    discovery.shutdown();
    discovery.reregister(&test_card("test", 0));
}

#[tokio::test]
async fn test_mdns_peer_discovery_fullname_starts_none() -> Result<()> {
    let mdns = test_mdns();
    assert!(mdns.fullname().is_none());
    Ok(())
}

#[tokio::test]
async fn test_mdns_peer_discovery_fullname_after_start() -> Result<()> {
    // We can't easily test the full start() path (needs a real daemon), but
    // we can verify that the fullname field is set correctly by calling
    // rename() which sets it.
    let mut mdns = test_mdns();
    // Without a daemon, rename() is a no-op and fullname stays None.
    mdns.rename(
        "old._nu-agent-a2a._tcp.local.",
        "new-name",
        12345,
        &test_card("new-name", 12345),
        "test-mesh",
    );
    assert!(mdns.fullname().is_none());
    Ok(())
}

#[tokio::test]
async fn reregister_mdns_when_not_started_is_noop() {
    let mut discovery = PeerDiscoveryImpl::Mdns(Box::new(test_mdns()));
    discovery.reregister(&test_card("test", 0));
}

#[tokio::test]
async fn rediscover_mdns_when_not_started_returns_none() {
    // -- Setup & Fixtures
    let mut discovery = PeerDiscoveryImpl::Mdns(Box::new(test_mdns()));

    // -- Exec
    let rx = discovery.rediscover();

    // -- Check
    assert!(rx.is_none());
}
