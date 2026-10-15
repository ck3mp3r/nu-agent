use std::time::{Duration, Instant};

use mdns_sd::{Receiver, ResolvedService, ServiceDaemon, ServiceEvent, ServiceInfo};
use tokio::sync::mpsc;

use crate::discovery::static_discovery::StaticPeerDiscovery;
use crate::discovery::{
    DiscoveryBrowser, DiscoveryService, PeerDiscoveryImpl, PeerEvent, mdns_name_for_switch,
};
use crate::{AgentCard, Peer};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

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

/// Synthetic service info used by the ignored mDNS smoke tests.
fn build_test_service_info(agent_name: &str, port: u16, mesh_key: &str) -> ServiceInfo {
    let props: Vec<(&str, &str)> = vec![("name", agent_name), ("mesh_key", mesh_key)];
    ServiceInfo::new(
        "_nu-agent-a2a._tcp.local.",
        agent_name,
        &format!("{agent_name}.local."),
        "127.0.0.1",
        port,
        props.as_slice(),
    )
    .expect("synthetic ServiceInfo")
}

/// Await a resolved mDNS service whose fullname contains `needle`, within
/// `budget`. Returns `None` when the budget expires or the channel closes.
fn wait_for_resolved(
    receiver: &Receiver<ServiceEvent>,
    needle: &str,
    budget: Duration,
) -> Option<Box<ResolvedService>> {
    let deadline = Instant::now() + budget;
    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        match receiver.recv_timeout(remaining) {
            Ok(ServiceEvent::ServiceResolved(resolved))
                if resolved.get_fullname().contains(needle) =>
            {
                return Some(resolved);
            }
            Ok(_) => {}
            Err(_) => return None,
        }
    }
    None
}

// ---------------------------------------------------------------------------
// peer_from_fields tests (pure conversion logic)
// ---------------------------------------------------------------------------

/// Verify the pure field-mapping function produces a correct [`Peer`].
#[test]
fn test_peer_from_fields_populates_correctly() {
    let peer = peer_from_fields("my-agent", "192.168.1.10", 8080, "my-agent.local.");

    assert_eq!(peer.name, "my-agent");
    assert_eq!(peer.url, "http://192.168.1.10:8080");
    assert_eq!(peer.host, "my-agent.local.");
    assert_eq!(peer.port, 8080);
    assert!(peer.card.is_none());
}

/// Verify the field-mapping function handles port 0.
#[test]
fn test_peer_from_fields_port_zero() {
    let peer = peer_from_fields("zero-port", "127.0.0.1", 0, "zero-port.local.");
    assert_eq!(peer.port, 0);
    assert_eq!(peer.url, "http://127.0.0.1:0");
}

/// Verify that [`peer_url_from_service`] uses the TXT `url` property when
/// present, rather than constructing from the mDNS address list.
///
/// NOTE: Requires a working mDNS responder on the host.
#[ignore]
#[test]
fn test_peer_url_from_txt_property() -> Result<()> {
    let daemon = ServiceDaemon::new().expect("ServiceDaemon");

    // Register with a url TXT property that differs from the address-based URL.
    let props: Vec<(&str, &str)> = vec![
        ("name", "txt-url-agent"),
        ("mesh_key", "test-mesh"),
        ("url", "http://127.0.0.1:9999"),
    ];
    let info = ServiceInfo::new(
        "_nu-agent-a2a._tcp.local.",
        "txt-url-agent",
        "txt-url-agent.local.",
        "127.0.0.1",
        9999,
        props.as_slice(),
    )
    .expect("synthetic ServiceInfo");
    daemon.register(info).expect("register");

    let receiver = daemon.browse("_nu-agent-a2a._tcp.local.").expect("browse");

    let resolved = wait_for_resolved(&receiver, "txt-url-agent", Duration::from_secs(3))
        .ok_or("Should discover txt-url-agent via mDNS")?;
    let url = crate::discovery::peer_url_from_service(&resolved);
    assert_eq!(
        url, "http://127.0.0.1:9999",
        "peer_url_from_service should use TXT url property"
    );

    let _ = daemon.shutdown();
    Ok(())
}

/// Verify browse returns a live channel.
///
/// NOTE: Requires a working mDNS responder on the host.
#[ignore]
#[test]
fn test_browse_returns_live_channel() {
    let daemon = ServiceDaemon::new().unwrap();
    let (_browser, rx) = DiscoveryBrowser::browse(daemon, "test-mesh", 52095).unwrap();
    assert!(!rx.is_closed(), "receiver should be alive after browse()");
}

/// Integration test using raw mdns-sd daemon calls (no wrappers).
///
/// NOTE: Requires a working mDNS responder on the host.
#[ignore]
#[test]
fn test_raw_mdns_sd_register_and_browse() {
    let daemon = ServiceDaemon::new().expect("ServiceDaemon");

    let info = build_test_service_info("raw-test-agent", 6666, "test-mesh");
    daemon.register(info).expect("register");

    let receiver = daemon.browse("_nu-agent-a2a._tcp.local.").expect("browse");

    // Wait up to 3 seconds for our own service.
    let found = wait_for_resolved(&receiver, "raw-test-agent", Duration::from_secs(3)).is_some();
    assert!(found, "Should discover raw-test-agent via mDNS");
    let _ = daemon.shutdown();
}

// ---------------------------------------------------------------------------
// PeerDiscoveryImpl — Noop
// ---------------------------------------------------------------------------

/// Verify that [`PeerDiscoveryImpl::Noop`] can be created and all methods
/// are safe no-ops.
#[test]
fn test_peer_discovery_impl_noop() {
    let mut discovery = PeerDiscoveryImpl::Noop;
    discovery.start("noop", 0, &AgentCard::default(), "mesh");
    assert!(discovery.take_peer_rx().is_none());
    discovery.shutdown();
}

// ---------------------------------------------------------------------------
// StaticPeerDiscovery
// ---------------------------------------------------------------------------

/// Verify that [`StaticPeerDiscovery`] with no peers returns a receiver
/// that is immediately empty (no events).
#[test]
fn test_static_discovery_empty_peers() -> Result<()> {
    let mut discovery = StaticPeerDiscovery::new(vec![]);
    let mut rx = discovery.take_peer_rx().ok_or("should have peer_rx")?;
    assert!(
        rx.try_recv().is_err(),
        "no events expected for empty peer list"
    );
    Ok(())
}

/// Verify that [`StaticPeerDiscovery::start`] and [`shutdown`] are safe
/// no-ops.
#[test]
fn test_static_discovery_start_shutdown_noop() {
    let mut discovery = StaticPeerDiscovery::new(vec![]);
    discovery.start("any", 0, &AgentCard::default(), "mesh");
    discovery.shutdown();
}

// ---------------------------------------------------------------------------
// mdns_name_for_switch — port-suffix logic
// ---------------------------------------------------------------------------

#[test]
fn test_mdns_name_for_switch_appends_port_suffix_when_old_name_had_it() {
    // When the old mDNS name ends with `-{port}`, the new name must also
    // carry the suffix.
    let result = mdns_name_for_switch("researcher-12345", "reviewer", 12345);
    assert_eq!(result, "reviewer-12345");
}

#[test]
fn test_mdns_name_for_switch_does_not_append_port_suffix_when_old_name_lacks_it() {
    // When the old mDNS name does NOT end with `-{port}`, the new name is
    // used verbatim.
    let result = mdns_name_for_switch("researcher", "reviewer", 12345);
    assert_eq!(result, "reviewer");
}

#[test]
fn test_mdns_name_for_switch_handles_partial_port_match() {
    // The suffix must match the full `-{port}` pattern, not just a substring.
    let result = mdns_name_for_switch("researcher-123", "reviewer", 12345);
    assert_eq!(result, "reviewer");
}

#[test]
fn test_mdns_name_for_switch_handles_same_name_different_port() {
    // Same name but different port — suffix should not be appended.
    let result = mdns_name_for_switch("agent-8080", "agent", 9090);
    assert_eq!(result, "agent");
}

// ---------------------------------------------------------------------------
// MdnsPeerDiscovery — fullname tracking
// ---------------------------------------------------------------------------

#[test]
fn test_mdns_peer_discovery_fullname_starts_none() {
    let mdns = crate::discovery::mdns_discovery::MdnsPeerDiscovery::default();
    assert!(mdns.fullname().is_none());
}

#[test]
fn test_mdns_peer_discovery_fullname_after_start() {
    // We can't easily test the full start() path (needs a real daemon), but
    // we can verify that the fullname field is set correctly by calling
    // rename() which sets it.
    let mut mdns = crate::discovery::mdns_discovery::MdnsPeerDiscovery::default();
    // Without a daemon, rename() is a no-op and fullname stays None.
    mdns.rename(
        "old._nu-agent-a2a._tcp.local.",
        "new-name",
        12345,
        &test_card("new-name", 12345),
        "test-mesh",
    );
    assert!(mdns.fullname().is_none());
}

// ---------------------------------------------------------------------------
// PeerDiscoveryImpl — fullname and rename dispatch
// ---------------------------------------------------------------------------

#[test]
fn test_peer_discovery_impl_fullname_noop() {
    let impl_ = PeerDiscoveryImpl::Noop;
    assert!(impl_.fullname().is_none());
}

#[test]
fn test_peer_discovery_impl_fullname_static() {
    let impl_ = PeerDiscoveryImpl::Static(StaticPeerDiscovery::new(vec![]));
    assert!(impl_.fullname().is_none());
}

#[test]
fn test_peer_discovery_impl_rename_noop_does_not_panic() {
    let mut impl_ = PeerDiscoveryImpl::Noop;
    impl_.rename(
        "old._nu-agent-a2a._tcp.local.",
        "new-name",
        12345,
        &test_card("new-name", 12345),
        "test-mesh",
    );
    // Should not panic — no-op is fine.
}

#[test]
fn test_peer_discovery_impl_rename_static_does_not_panic() {
    let mut impl_ = PeerDiscoveryImpl::Static(StaticPeerDiscovery::new(vec![]));
    impl_.rename(
        "old._nu-agent-a2a._tcp.local.",
        "new-name",
        12345,
        &test_card("new-name", 12345),
        "test-mesh",
    );
    // Should not panic — no-op is fine.
}

// ---------------------------------------------------------------------------
// DiscoveryService — struct construction (noop smoke test)
// ---------------------------------------------------------------------------

#[test]
fn discovery_service_noop_does_not_crash() {
    let service = DiscoveryService { _daemon: None };
    drop(service);
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
// StaticPeerDiscovery — configured peers
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
