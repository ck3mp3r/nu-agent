//! Pending-decision resolution: the oneshot registered for a request.

use std::sync::Arc;

use agnt_core::protocol::event::PermissionDecision;

use super::support::*;
use crate::permission::AcpPermissionBridge;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[tokio::test]
async fn test_permission_bridge_resolves_pending_oneshot_with_decision() -> Result<()> {
    // -- Setup & Fixtures
    let sink = RecordingPermissionSink::new(selected("allow-always"));
    let pending = empty_pending();
    let rx = register_pending(&pending, REQUEST);
    let mut bridge = AcpPermissionBridge::new(sink, SESSION, pending);

    // -- Exec
    let decision = bridge.handle_permission_request(REQUEST, context()).await?;
    let resolved = rx.await?;

    // -- Check
    assert_eq!(decision, PermissionDecision::AllowAlways);
    assert_eq!(resolved, PermissionDecision::AllowAlways);
    Ok(())
}

#[tokio::test]
async fn test_permission_bridge_resolves_pending_oneshot_with_deny_on_reject() -> Result<()> {
    // -- Setup & Fixtures
    let sink = RecordingPermissionSink::new(selected("reject-once"));
    let pending = empty_pending();
    let rx = register_pending(&pending, REQUEST);
    let mut bridge = AcpPermissionBridge::new(sink, SESSION, pending);

    // -- Exec
    let decision = bridge.handle_permission_request(REQUEST, context()).await?;
    let resolved = rx.await?;

    // -- Check
    assert_eq!(decision, PermissionDecision::Deny);
    assert_eq!(resolved, PermissionDecision::Deny);
    Ok(())
}

#[tokio::test]
async fn test_permission_bridge_without_pending_entry_still_returns_decision() -> Result<()> {
    // -- Setup & Fixtures
    let sink = RecordingPermissionSink::new(selected("allow-once"));
    let mut bridge = AcpPermissionBridge::new(sink, SESSION, empty_pending());

    // -- Exec
    let decision = bridge
        .handle_permission_request("unregistered", context())
        .await?;

    // -- Check
    assert_eq!(decision, PermissionDecision::AllowOnce);
    Ok(())
}

#[tokio::test]
async fn test_permission_bridge_denied_key_resolves_pending_oneshot_with_deny() -> Result<()> {
    // -- Setup & Fixtures
    let sink = RecordingPermissionSink::new(selected("reject-always"));
    let pending = empty_pending();
    let mut bridge = AcpPermissionBridge::new(sink, SESSION, Arc::clone(&pending));
    let first = bridge.handle_permission_request(REQUEST, context()).await?;
    assert_eq!(first, PermissionDecision::Deny);
    let rx = register_pending(&pending, "req-2");

    // -- Exec
    let second = bridge.handle_permission_request("req-2", context()).await?;
    let resolved = rx.await?;

    // -- Check
    assert_eq!(second, PermissionDecision::Deny);
    assert_eq!(resolved, PermissionDecision::Deny);
    Ok(())
}
