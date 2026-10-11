//! Deny set: `reject-always` memory and auto-deny for the rest of the session.

use agnt_core::protocol::event::PermissionDecision;

use super::support::*;
use crate::permission::AcpPermissionBridge;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[tokio::test]
async fn test_permission_bridge_is_denied_false_for_unknown_key() -> Result<()> {
    // -- Setup & Fixtures
    let sink = RecordingPermissionSink::new(selected("allow-once"));
    let mut bridge = AcpPermissionBridge::new(sink, SESSION, empty_pending());

    // -- Exec
    let decision = bridge.handle_permission_request(REQUEST, context()).await?;
    let denied = bridge.is_denied(TOOL_KEY);

    // -- Check
    assert_eq!(decision, PermissionDecision::AllowOnce);
    assert!(!denied, "an allowed call must not enter the deny set");
    Ok(())
}

#[tokio::test]
async fn test_permission_bridge_reject_always_adds_key_to_deny_set() -> Result<()> {
    // -- Setup & Fixtures
    let sink = RecordingPermissionSink::new(selected("reject-always"));
    let mut bridge = AcpPermissionBridge::new(sink, SESSION, empty_pending());

    // -- Exec
    let decision = bridge.handle_permission_request(REQUEST, context()).await?;
    let denied = bridge.is_denied(TOOL_KEY);

    // -- Check
    assert_eq!(decision, PermissionDecision::Deny);
    assert!(denied, "reject-always must deny the key for the session");
    Ok(())
}

#[tokio::test]
async fn test_permission_bridge_reject_once_does_not_add_key_to_deny_set() -> Result<()> {
    // -- Setup & Fixtures
    let sink = RecordingPermissionSink::new(selected("reject-once"));
    let mut bridge = AcpPermissionBridge::new(sink, SESSION, empty_pending());

    // -- Exec
    let decision = bridge.handle_permission_request(REQUEST, context()).await?;
    let denied = bridge.is_denied(TOOL_KEY);

    // -- Check
    assert_eq!(decision, PermissionDecision::Deny);
    assert!(!denied, "reject-once must not deny the key for the session");
    Ok(())
}

#[tokio::test]
async fn test_permission_bridge_denied_key_is_denied_without_asking_sink() -> Result<()> {
    // -- Setup & Fixtures
    let sink = RecordingPermissionSink::new(selected("reject-always"));
    let recorder = sink.clone();
    let mut bridge = AcpPermissionBridge::new(sink, SESSION, empty_pending());
    let first = bridge.handle_permission_request(REQUEST, context()).await?;
    assert_eq!(first, PermissionDecision::Deny);

    // -- Exec
    let second = bridge.handle_permission_request("req-2", context()).await?;

    // -- Check
    assert_eq!(second, PermissionDecision::Deny);
    assert_eq!(
        recorder.requests().len(),
        1,
        "a denied key must not reach the sink a second time"
    );
    Ok(())
}

#[tokio::test]
async fn test_permission_bridge_other_key_still_asks_sink_after_reject_always() -> Result<()> {
    // -- Setup & Fixtures
    let sink = RecordingPermissionSink::new(selected("reject-always"));
    let recorder = sink.clone();
    let mut bridge = AcpPermissionBridge::new(sink, SESSION, empty_pending());
    let first = bridge.handle_permission_request(REQUEST, context()).await?;
    assert_eq!(first, PermissionDecision::Deny);

    // -- Exec
    let other = bridge
        .handle_permission_request("req-2", context_for(OTHER_TOOL_KEY))
        .await?;

    // -- Check
    assert_eq!(other, PermissionDecision::Deny);
    let requests = recorder.requests();
    assert_eq!(
        requests.len(),
        2,
        "a different key must still reach the sink"
    );
    assert_eq!(
        requests[1].tool_call.tool_call_id.0.as_ref(),
        OTHER_TOOL_KEY
    );
    Ok(())
}

#[tokio::test]
async fn test_permission_bridge_deny_set_is_scoped_to_one_key() -> Result<()> {
    // -- Setup & Fixtures
    let sink = RecordingPermissionSink::new(selected("reject-always"));
    let mut bridge = AcpPermissionBridge::new(sink, SESSION, empty_pending());
    let first = bridge.handle_permission_request(REQUEST, context()).await?;
    assert_eq!(first, PermissionDecision::Deny);

    // -- Exec
    let denied = bridge.is_denied(TOOL_KEY);
    let other_denied = bridge.is_denied(OTHER_TOOL_KEY);

    // -- Check
    assert!(denied, "the rejected key must be denied");
    assert!(!other_denied, "an unrelated key must not be denied");
    Ok(())
}
