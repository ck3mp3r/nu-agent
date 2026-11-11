//! The ignored decision: an answer for a request that is no longer pending.

use agnt_runtime::bus::{Bus, ControlCommand};
use agnt_runtime::protocol::event::PermissionDecision;

use super::support::*;
use crate::permission::AcpPermissionBridge;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[tokio::test]
async fn test_permission_bridge_unknown_request_publishes_decision() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let mut rx = bus.control().subscribe();
    let sink = RecordingPermissionSink::new(selected("allow-once"));
    let mut bridge = AcpPermissionBridge::new(sink, SESSION, empty_pending())
        .with_control(bus.control().clone());

    // -- Exec
    let decision = bridge
        .handle_permission_request(&card_with_request("unregistered"))
        .await?;

    // -- Check
    assert_eq!(decision, PermissionDecision::AllowOnce);
    match rx.try_recv() {
        Ok(ControlCommand::PermissionDecided {
            request_id,
            decision,
        }) => {
            assert_eq!(request_id.as_str(), "unregistered");
            assert_eq!(decision, PermissionDecision::AllowOnce);
        }
        other => {
            return Err(format!("expected PermissionDecided, got {other:?}").into());
        }
    }
    assert!(
        rx.try_recv().is_err(),
        "exactly one command must be published for one unanswered decision"
    );
    Ok(())
}

#[tokio::test]
async fn test_permission_bridge_without_control_publishes_nothing() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let mut rx = bus.control().subscribe();
    let sink = RecordingPermissionSink::new(selected("allow-once"));
    let mut bridge = AcpPermissionBridge::new(sink, SESSION, empty_pending());

    // -- Exec
    let decision = bridge
        .handle_permission_request(&card_with_request("unregistered"))
        .await?;

    // -- Check
    assert_eq!(decision, PermissionDecision::AllowOnce);
    assert!(
        rx.try_recv().is_err(),
        "a bridge built without with_control must publish no command"
    );
    Ok(())
}

#[tokio::test]
async fn test_permission_bridge_pending_request_publishes_no_decision() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let mut rx = bus.control().subscribe();
    let sink = RecordingPermissionSink::new(selected("allow-once"));
    let pending = empty_pending();
    let resolved = register_pending(&pending, REQUEST);
    let mut bridge =
        AcpPermissionBridge::new(sink, SESSION, pending).with_control(bus.control().clone());

    // -- Exec
    let decision = bridge.handle_permission_request(&card()).await?;
    let resolved = resolved.await?;

    // -- Check
    assert_eq!(decision, PermissionDecision::AllowOnce);
    assert_eq!(resolved, PermissionDecision::AllowOnce);
    assert!(
        rx.try_recv().is_err(),
        "a resolved pending request must not publish a decision command"
    );
    Ok(())
}
