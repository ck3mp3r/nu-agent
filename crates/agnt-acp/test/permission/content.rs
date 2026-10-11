//! Permission request content: the diff forwarded to the client.

use agent_client_protocol::schema::v1::ToolCallContent;
use agnt_core::protocol::event::PermissionDecision;

use super::support::*;
use crate::permission::AcpPermissionBridge;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[tokio::test]
async fn test_permission_bridge_forwards_diff_content_when_display_present() -> Result<()> {
    // -- Setup & Fixtures
    let sink = RecordingPermissionSink::new(selected("allow-once"));
    let recorder = sink.clone();
    let mut bridge = AcpPermissionBridge::new(sink, SESSION, empty_pending());

    // -- Exec
    let decision = bridge
        .handle_permission_request(REQUEST, context_with_display(diff_display()))
        .await?;

    // -- Check
    assert_eq!(decision, PermissionDecision::AllowOnce);
    let requests = recorder.requests();
    assert_eq!(requests.len(), 1);
    let content = requests[0]
        .tool_call
        .fields
        .content
        .as_ref()
        .ok_or("content must be present when a display is forwarded")?;
    assert_eq!(content.len(), 1);
    let ToolCallContent::Diff(diff) = &content[0] else {
        return Err("expected a diff content block".into());
    };
    assert_eq!(diff.path.to_string_lossy(), "src/lib.rs");
    assert_eq!(diff.old_text.as_deref(), Some("old line\ncontext\n"));
    assert_eq!(diff.new_text, "new line\ncontext\n");
    Ok(())
}

#[tokio::test]
async fn test_permission_bridge_omits_content_when_no_display() -> Result<()> {
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
    assert!(
        requests[0].tool_call.fields.content.is_none(),
        "content must be absent when no display is forwarded"
    );
    Ok(())
}
