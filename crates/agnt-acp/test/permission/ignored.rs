//! The ignored-decision event: a decision for a request that is not pending.

use agnt_core::bus::create_bus;
use agnt_core::protocol::event::PermissionDecision;
use agnt_core::protocol::events::SystemEvent;

use super::support::*;
use crate::permission::AcpPermissionBridge;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[tokio::test]
async fn test_permission_bridge_unknown_request_publishes_ignored() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let mut rx = bus.ui_event().subscribe();
    let sink = RecordingPermissionSink::new(selected("allow-once"));
    let mut bridge = AcpPermissionBridge::new(sink, SESSION, empty_pending())
        .with_ui_events(bus.ui_event().clone());

    // -- Exec
    let decision = bridge
        .handle_permission_request("unregistered", context())
        .await?;

    // -- Check
    assert_eq!(decision, PermissionDecision::AllowOnce);
    match rx.try_recv() {
        Ok(SystemEvent::PermissionDecisionIgnored { request_id, reason }) => {
            assert_eq!(request_id.as_str(), "unregistered");
            assert_eq!(reason, "request_not_pending");
        }
        other => {
            return Err(format!("expected PermissionDecisionIgnored, got {other:?}").into());
        }
    }
    assert!(
        rx.try_recv().is_err(),
        "exactly one event must be published for one ignored decision"
    );
    Ok(())
}

#[tokio::test]
async fn test_permission_bridge_without_ui_events_publishes_nothing() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let mut rx = bus.ui_event().subscribe();
    let sink = RecordingPermissionSink::new(selected("allow-once"));
    let mut bridge = AcpPermissionBridge::new(sink, SESSION, empty_pending());

    // -- Exec
    let decision = bridge
        .handle_permission_request("unregistered", context())
        .await?;

    // -- Check
    assert_eq!(decision, PermissionDecision::AllowOnce);
    assert!(
        rx.try_recv().is_err(),
        "a bridge built without with_ui_events must publish no event"
    );
    Ok(())
}

#[tokio::test]
async fn test_permission_bridge_pending_request_publishes_no_ignored() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let mut rx = bus.ui_event().subscribe();
    let sink = RecordingPermissionSink::new(selected("allow-once"));
    let pending = empty_pending();
    let resolved = register_pending(&pending, REQUEST);
    let mut bridge =
        AcpPermissionBridge::new(sink, SESSION, pending).with_ui_events(bus.ui_event().clone());

    // -- Exec
    let decision = bridge.handle_permission_request(REQUEST, context()).await?;
    let resolved = resolved.await?;

    // -- Check
    assert_eq!(decision, PermissionDecision::AllowOnce);
    assert_eq!(resolved, PermissionDecision::AllowOnce);
    assert!(
        rx.try_recv().is_err(),
        "a resolved pending request must not publish an ignored event"
    );
    Ok(())
}
