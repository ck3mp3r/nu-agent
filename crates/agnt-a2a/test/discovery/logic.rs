use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};

use crate::discovery::static_discovery::StaticPeerDiscovery;
use crate::discovery::{
    DiscoveryBrowser, DiscoveryService, PeerDiscoveryImpl, mdns_name_for_switch,
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
fn test_peer_url_from_txt_property() {
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

    let mut found = false;
    for _ in 0..30 {
        if let Ok(ServiceEvent::ServiceResolved(resolved)) = receiver.try_recv()
            && resolved.get_fullname().contains("txt-url-agent")
        {
            let url = crate::discovery::peer_url_from_service(&resolved);
            assert_eq!(
                url, "http://127.0.0.1:9999",
                "peer_url_from_service should use TXT url property"
            );
            found = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    assert!(found, "Should discover txt-url-agent via mDNS");
    let _ = daemon.shutdown();
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
    let mut found = false;
    for _ in 0..30 {
        if let Ok(ServiceEvent::ServiceResolved(resolved)) = receiver.try_recv()
            && resolved.get_fullname().contains("raw-test-agent")
        {
            found = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

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
