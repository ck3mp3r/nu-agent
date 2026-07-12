use tokio::sync::mpsc;

use super::*;
use zeroconf::ServiceType;
use zeroconf::browser::{BrowserEvent, ServiceDiscovery, ServiceRemoval};
use zeroconf::prelude::*;

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

/// Build a synthetic `ServiceDiscovery` suitable for testing.
fn make_service_discovery(name: &str, port: u16) -> ServiceDiscovery {
    ServiceDiscovery::builder()
        .name(name.to_string())
        .address("127.0.0.1".to_string())
        .port(port)
        .host_name(format!("{name}.local."))
        .domain("local".to_string())
        .txt(None)
        .service_type(ServiceType::new("nu-agent-a2a", "tcp").unwrap())
        .build()
        .expect("synthetic ServiceDiscovery")
}

// ---------------------------------------------------------------------------
// process_zeroconf_event — synthetic event processing tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_process_discovery_add_event() {
    let (tx, mut rx) = mpsc::channel::<PeerEvent>(16);

    let service = make_service_discovery("test-agent", 5001);
    process_zeroconf_event(&BrowserEvent::Add(service), &tx);

    let event = rx.try_recv().expect("should have received PeerEvent");

    match event {
        PeerEvent::PeerDiscovered(peer) => {
            assert!(
                peer.name.contains("test-agent"),
                "name should contain test-agent"
            );
            assert_eq!(peer.port, 5001);
            assert_eq!(peer.host, "test-agent.local.");
        }
        other => panic!("Expected PeerDiscovered, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_process_discovery_add_multiple() {
    let (tx, mut rx) = mpsc::channel::<PeerEvent>(16);

    let alpha = make_service_discovery("alpha", 5002);
    let beta = make_service_discovery("beta", 5003);

    process_zeroconf_event(&BrowserEvent::Add(alpha), &tx);
    process_zeroconf_event(&BrowserEvent::Add(beta), &tx);

    drop(tx);

    let mut discovered = std::collections::HashSet::new();
    while let Ok(Some(event)) =
        tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv()).await
    {
        if let PeerEvent::PeerDiscovered(peer) = event {
            discovered.insert(peer.name);
        }
    }

    assert!(
        discovered.iter().any(|n| n.contains("alpha")),
        "Should discover alpha"
    );
    assert!(
        discovered.iter().any(|n| n.contains("beta")),
        "Should discover beta"
    );
}

#[tokio::test]
async fn test_process_discovery_remove_event() {
    let (tx, mut rx) = mpsc::channel::<PeerEvent>(16);

    // Send a discovery event first, then a removal.
    let service = make_service_discovery("removable", 5004);
    process_zeroconf_event(&BrowserEvent::Add(service), &tx);

    let removal = ServiceRemoval::builder()
        .name("removable".to_string())
        .kind("_nu-agent-a2a._tcp".to_string())
        .domain("local".to_string())
        .build()
        .expect("synthetic ServiceRemoval");
    process_zeroconf_event(&BrowserEvent::Remove(removal), &tx);

    drop(tx);

    let mut discovered = false;
    let mut lost = false;

    while let Ok(Some(event)) =
        tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv()).await
    {
        match event {
            PeerEvent::PeerDiscovered(peer) => {
                if peer.name.contains("removable") {
                    discovered = true;
                }
            }
            PeerEvent::PeerLost(name) => {
                if name.contains("removable") {
                    lost = true;
                }
            }
        }
    }

    assert!(discovered, "Should discover removable");
    assert!(lost, "Should receive PeerLost for removable");
}

#[tokio::test]
async fn test_card_fetch_without_client() {
    // When no HTTP client/runtime is provided, card should be None.
    let (tx, mut rx) = mpsc::channel::<PeerEvent>(16);

    let service = make_service_discovery("no-client", 19999);
    process_zeroconf_event(&BrowserEvent::Add(service), &tx);

    drop(tx);

    let event = rx.try_recv().expect("should have received PeerEvent");

    match event {
        PeerEvent::PeerDiscovered(peer) => {
            assert!(peer.name.contains("no-client"));
            assert!(
                peer.card.is_none(),
                "Card should be None when no client is provided"
            );
        }
        other => panic!("Expected PeerDiscovered, got: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// DiscoveryService — registration (smoke tests)
// ---------------------------------------------------------------------------

/// Verify that we can create a service, register it, and drop it without
/// errors.
///
/// NOTE: This test requires a working mDNS responder (Bonjour/Avahi) on the
/// host. It is ignored by default in CI/offline environments.
#[ignore]
#[tokio::test]
async fn test_register_service() {
    let card = test_card("test-agent", 9999);
    let service = DiscoveryService::register("test-agent", 9999, &card).unwrap();
    drop(service);
}

/// Verify that registering the same name twice does not error.
///
/// NOTE: Requires a working mDNS responder (Bonjour/Avahi) on the host.
#[ignore]
#[tokio::test]
async fn test_register_duplicate_name() {
    let card = test_card("dup-agent", 3001);
    let a = DiscoveryService::register("dup-agent", 3001, &card).unwrap();
    let b = DiscoveryService::register("dup-agent", 3002, &card).unwrap();
    drop(a);
    drop(b);
}

// ---------------------------------------------------------------------------
// DiscoveryBrowser — smoke tests
// ---------------------------------------------------------------------------

/// Convert a synthetic zeroconf [`BrowserEvent`] into a [`PeerEvent`] and
/// forward it on the given channel.  Used by tests to simulate discovery
/// without a live mDNS responder.
fn process_zeroconf_event(event: &BrowserEvent, tx: &mpsc::Sender<PeerEvent>) {
    match event {
        BrowserEvent::Add(service) => {
            let peer = super::peer_from_service(service);

            let _ = tx.try_send(PeerEvent::PeerDiscovered(Box::new(peer)));
        }
        BrowserEvent::Remove(info) => {
            let _ = tx.try_send(PeerEvent::PeerLost(info.name().clone()));
        }
    }
}

/// Verify browse returns a live channel.
///
/// NOTE: Requires a working mDNS responder (Bonjour/Avahi) on the host.
#[ignore]
#[test]
fn test_browse_returns_live_channel() {
    let (_browser, rx) = DiscoveryBrowser::browse().unwrap();
    assert!(!rx.is_closed(), "receiver should be alive after browse()");
}
