//! ACP permission request test: a gated tool call round-trips through the client.

use std::sync::Arc;

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{InitializeRequest, StopReason};

use super::*;

// ---------------------------------------------------------------------------
// session/request_permission
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_integration_permission_request_allows_tool_call() -> Result<()> {
    // -- Setup & Fixtures
    let llm = start_llm_server().await?;
    mount_tool_then_text(
        &llm,
        "read",
        serde_json::json!({ "path": "note.txt" }),
        "read complete",
    )
    .await?;
    let env = TestEnv::new(&llm.uri(), "[permissions]\nread = \"ask\"\n")?;
    std::fs::write(env.cwd().join("note.txt"), "permission probe content")?;
    let recorder = Arc::new(Recorder::default());

    // -- Exec
    let (text, stop_reason) = build_client(Arc::clone(&recorder))
        .connect_with(env.agent(), async |cx| {
            cx.send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;
            let mut session = cx
                .build_session(env.cwd())
                .block_task()
                .start_session()
                .await?;
            session.send_prompt("read note.txt")?;
            let (text, stop_reason) = read_turn(&mut session).await.map_err(acp_err)?;
            Ok((text, stop_reason))
        })
        .await?;

    // -- Check
    let requests = recorder.permission_requests();
    assert_eq!(
        requests.len(),
        1,
        "the gated read tool must trigger exactly one permission request"
    );
    assert_eq!(stop_reason, StopReason::EndTurn);
    assert!(
        text.contains("read complete"),
        "the turn must continue after the tool call; got: {text:?}"
    );
    let bodies = llm_request_bodies(&llm).await;
    let second = bodies
        .get(1)
        .ok_or("the tool result must be fed back to the model as a second completion")?;
    assert!(
        second.contains("permission probe content"),
        "the second completion must carry the tool result; got: {second}"
    );
    Ok(())
}
