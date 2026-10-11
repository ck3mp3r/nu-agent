//! Request shape: the `session/request_permission` payload sent to the sink.

use agent_client_protocol::schema::v1::PermissionOptionKind;
use agnt_core::protocol::event::PermissionDecision;

use super::support::*;
use crate::permission::AcpPermissionBridge;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[tokio::test]
async fn test_permission_bridge_sends_four_options_to_sink() -> Result<()> {
    // -- Setup & Fixtures
    let sink = RecordingPermissionSink::new(selected("allow-once"));
    let recorder = sink.clone();
    let mut bridge = AcpPermissionBridge::new(sink, SESSION, empty_pending());

    // -- Exec
    let decision = bridge.handle_permission_request(REQUEST, context()).await?;

    // -- Check
    assert_eq!(decision, PermissionDecision::AllowOnce);
    let requests = recorder.requests();
    assert_eq!(requests.len(), 1, "exactly one request must reach the sink");
    let kinds: Vec<PermissionOptionKind> = requests[0]
        .options
        .iter()
        .map(|option| option.kind)
        .collect();
    assert_eq!(
        kinds,
        vec![
            PermissionOptionKind::AllowOnce,
            PermissionOptionKind::AllowAlways,
            PermissionOptionKind::RejectOnce,
            PermissionOptionKind::RejectAlways,
        ]
    );
    Ok(())
}

#[tokio::test]
async fn test_permission_bridge_sends_session_id_and_tool_call_key() -> Result<()> {
    // -- Setup & Fixtures
    let sink = RecordingPermissionSink::new(selected("allow-once"));
    let recorder = sink.clone();
    let mut bridge = AcpPermissionBridge::new(sink, SESSION, empty_pending());

    // -- Exec
    let decision = bridge.handle_permission_request(REQUEST, context()).await?;

    // -- Check
    assert_eq!(decision, PermissionDecision::AllowOnce);
    let requests = recorder.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].session_id, SESSION);
    assert_eq!(requests[0].tool_call.tool_call_id.0.as_ref(), TOOL_KEY);
    Ok(())
}
