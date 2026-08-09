use crate::AgentCard;
use crate::discovery::mdns_discovery::MdnsPeerDiscovery;

fn test_card(name: &str) -> AgentCard {
    AgentCard {
        name: name.to_string(),
        url: "http://127.0.0.1:0".to_string(),
        skills: vec![],
        ..Default::default()
    }
}

#[test]
fn reregister_does_not_panic_when_not_started() {
    let mut discovery = MdnsPeerDiscovery::new();
    discovery.reregister(&test_card("test"));
}

#[test]
fn reregister_preserves_fullname_when_no_daemon() {
    let mut discovery = MdnsPeerDiscovery::new();
    assert!(discovery.fullname().is_none());
    discovery.reregister(&test_card("test"));
    assert!(discovery.fullname().is_none());
}

#[test]
fn reregister_with_updated_card_does_not_panic() {
    let mut discovery = MdnsPeerDiscovery::new();
    let updated_card = AgentCard {
        description: Some("updated".to_string()),
        ..test_card("test")
    };
    discovery.reregister(&updated_card);
}

#[test]
fn shutdown_then_reregister_does_not_panic() {
    let mut discovery = MdnsPeerDiscovery::new();
    discovery.shutdown();
    discovery.reregister(&test_card("test"));
}
