//! Decision mapping: client outcome → `PermissionDecision`.

use agent_client_protocol::schema::v1::RequestPermissionOutcome;
use agnt_core::protocol::event::PermissionDecision;

use super::support::*;
use crate::permission::AcpPermissionBridge;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[tokio::test]
async fn test_permission_bridge_allow_once_returns_allow_once() -> Result<()> {
    // -- Setup & Fixtures
    let sink = RecordingPermissionSink::new(selected("allow-once"));
    let mut bridge = AcpPermissionBridge::new(sink, SESSION, empty_pending());

    // -- Exec
    let decision = bridge.handle_permission_request(REQUEST, context()).await?;

    // -- Check
    assert_eq!(decision, PermissionDecision::AllowOnce);
    Ok(())
}

#[tokio::test]
async fn test_permission_bridge_allow_always_returns_allow_always() -> Result<()> {
    // -- Setup & Fixtures
    let sink = RecordingPermissionSink::new(selected("allow-always"));
    let mut bridge = AcpPermissionBridge::new(sink, SESSION, empty_pending());

    // -- Exec
    let decision = bridge.handle_permission_request(REQUEST, context()).await?;

    // -- Check
    assert_eq!(decision, PermissionDecision::AllowAlways);
    Ok(())
}

#[tokio::test]
async fn test_permission_bridge_reject_once_returns_deny() -> Result<()> {
    // -- Setup & Fixtures
    let sink = RecordingPermissionSink::new(selected("reject-once"));
    let mut bridge = AcpPermissionBridge::new(sink, SESSION, empty_pending());

    // -- Exec
    let decision = bridge.handle_permission_request(REQUEST, context()).await?;

    // -- Check
    assert_eq!(decision, PermissionDecision::Deny);
    Ok(())
}

#[tokio::test]
async fn test_permission_bridge_reject_always_returns_deny() -> Result<()> {
    // -- Setup & Fixtures
    let sink = RecordingPermissionSink::new(selected("reject-always"));
    let mut bridge = AcpPermissionBridge::new(sink, SESSION, empty_pending());

    // -- Exec
    let decision = bridge.handle_permission_request(REQUEST, context()).await?;

    // -- Check
    assert_eq!(decision, PermissionDecision::Deny);
    Ok(())
}

#[tokio::test]
async fn test_permission_bridge_cancelled_outcome_returns_deny() -> Result<()> {
    // -- Setup & Fixtures
    let sink = RecordingPermissionSink::new(RequestPermissionOutcome::Cancelled);
    let mut bridge = AcpPermissionBridge::new(sink, SESSION, empty_pending());

    // -- Exec
    let decision = bridge.handle_permission_request(REQUEST, context()).await?;

    // -- Check
    assert_eq!(decision, PermissionDecision::Deny);
    Ok(())
}

#[tokio::test]
async fn test_permission_bridge_unknown_option_returns_deny() -> Result<()> {
    // -- Setup & Fixtures
    let sink = RecordingPermissionSink::new(selected("some-client-specific-option"));
    let mut bridge = AcpPermissionBridge::new(sink, SESSION, empty_pending());

    // -- Exec
    let decision = bridge.handle_permission_request(REQUEST, context()).await?;

    // -- Check
    assert_eq!(decision, PermissionDecision::Deny);
    Ok(())
}
