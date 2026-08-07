use super::*;

#[test]
fn discovery_service_noop_does_not_crash() {
    let service = DiscoveryService { _daemon: None };
    drop(service);
}
