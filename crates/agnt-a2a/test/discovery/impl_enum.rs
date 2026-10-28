use super::*;
use crate::AgentCard;
use crate::discovery::static_discovery::StaticPeerDiscovery;

fn test_card(name: &str) -> AgentCard {
    AgentCard {
        name: name.to_string(),
        url: "http://127.0.0.1:0".to_string(),
        skills: vec![],
        ..Default::default()
    }
}

#[test]
fn reregister_noop_does_not_crash() {
    let mut discovery = PeerDiscoveryImpl::Noop;
    discovery.reregister(&test_card("test"));
}

#[test]
fn reregister_static_does_not_crash() {
    let mut discovery = PeerDiscoveryImpl::Static(StaticPeerDiscovery::new(vec![]));
    discovery.reregister(&test_card("test"));
}

#[test]
fn reregister_mdns_when_not_started_is_noop() {
    let mut discovery = PeerDiscoveryImpl::Mdns(Box::default());
    discovery.reregister(&test_card("test"));
}

#[test]
fn rediscover_noop_returns_none() {
    // -- Setup & Fixtures
    let mut discovery = PeerDiscoveryImpl::Noop;

    // -- Exec
    let rx = discovery.rediscover();

    // -- Check
    assert!(rx.is_none());
}

#[test]
fn rediscover_static_returns_none() {
    // -- Setup & Fixtures
    let mut discovery = PeerDiscoveryImpl::Static(StaticPeerDiscovery::new(vec![]));

    // -- Exec
    let rx = discovery.rediscover();

    // -- Check
    assert!(rx.is_none());
}

#[test]
fn rediscover_mdns_when_not_started_returns_none() {
    // -- Setup & Fixtures
    let mut discovery = PeerDiscoveryImpl::Mdns(Box::default());

    // -- Exec
    let rx = discovery.rediscover();

    // -- Check
    assert!(rx.is_none());
}
