//! End-to-end ACP stdio integration tests.
//!
//! Every test launches `agent acp` as a real subprocess through the
//! ACP SDK client and drives the protocol over stdio. The LLM is a local
//! wiremock server, so no API key and no network access are required.

use std::sync::Arc;
use std::time::Duration;

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    CancelNotification, DeleteSessionRequest, InitializeRequest, ListSessionsRequest,
    SessionConfigKind, SessionConfigOptionCategory, StopReason,
};

mod support;
use support::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// initialize -> session/new -> session/prompt
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_integration_initialize_new_session_prompt_end_turn() -> Result<()> {
    // -- Setup & Fixtures
    let llm = start_llm_server().await?;
    mount_text_response(&llm, "hello from mock", None).await?;
    let env = TestEnv::new(&llm.uri(), "")?;
    let recorder = Arc::new(Recorder::default());

    // -- Exec
    let (agent_name, load_session, session_id, text, stop_reason) =
        build_client(Arc::clone(&recorder))
            .connect_with(env.agent(), async |cx| {
                let init = cx
                    .send_request(InitializeRequest::new(ProtocolVersion::V1))
                    .block_task()
                    .await?;
                let mut session = cx
                    .build_session(env.cwd())
                    .block_task()
                    .start_session()
                    .await?;
                let session_id = session.session_id().to_string();
                session.send_prompt("hello")?;
                let (text, stop_reason) = read_turn(&mut session).await.map_err(acp_err)?;
                Ok((
                    init.agent_info.map(|info| info.name),
                    init.agent_capabilities.load_session,
                    session_id,
                    text,
                    stop_reason,
                ))
            })
            .await?;

    // -- Check
    assert_eq!(agent_name.as_deref(), Some("nu-agent"));
    assert!(load_session, "the agent must advertise session/load");
    assert!(!session_id.is_empty(), "session/new must return an id");
    assert_eq!(stop_reason, StopReason::EndTurn);
    assert!(
        text.contains("hello from mock"),
        "the turn must surface the mock completion; got: {text:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// session/cancel
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_integration_session_cancel_stops_in_flight_prompt() -> Result<()> {
    // -- Setup & Fixtures
    let llm = start_llm_server().await?;
    mount_text_response(&llm, "slow response", Some(Duration::from_secs(3))).await?;
    let env = TestEnv::new(&llm.uri(), "")?;
    let recorder = Arc::new(Recorder::default());

    // -- Exec
    let stop_reason = build_client(recorder)
        .connect_with(env.agent(), async |cx| {
            cx.send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;
            let mut session = cx
                .build_session(env.cwd())
                .block_task()
                .start_session()
                .await?;
            session.send_prompt("hello")?;
            // Wait until the turn's completion request reaches the mock server,
            // so the cancel lands while the prompt is genuinely in flight.
            let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
            while llm_request_count(&llm).await == 0 {
                if tokio::time::Instant::now() >= deadline {
                    return Err(acp_err("the turn never reached the mock LLM"));
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            cx.send_notification(CancelNotification::new(session.session_id().clone()))?;
            let (_, stop_reason) = read_turn(&mut session).await.map_err(acp_err)?;
            Ok(stop_reason)
        })
        .await?;

    // -- Check
    assert_eq!(
        stop_reason,
        StopReason::Cancelled,
        "session/cancel must make the prompt report stopReason: cancelled"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// session/load
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_integration_session_load_replays_history() -> Result<()> {
    // -- Setup & Fixtures
    let llm = start_llm_server().await?;
    mount_text_response(&llm, "hi there", None).await?;
    let env = TestEnv::new(&llm.uri(), "")?;
    let recorder = Arc::new(Recorder::default());

    // -- Exec
    let (stop_reason, chunks) = build_client(recorder)
        .connect_with(env.agent(), async |cx| {
            cx.send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;
            let mut session = cx
                .build_session(env.cwd())
                .block_task()
                .start_session()
                .await?;
            let session_id = session.session_id().clone();
            session.send_prompt("hello")?;
            let (_, stop_reason) = read_turn(&mut session).await.map_err(acp_err)?;
            drop(session);

            let restored = cx
                .load_session(session_id, env.cwd())
                .block_task()
                .start_session()
                .await?;
            let mut restored = restored.into_session();
            let chunks = collect_replay(&mut restored, Duration::from_secs(2))
                .await
                .map_err(acp_err)?;
            Ok((stop_reason, chunks))
        })
        .await?;

    // -- Check
    assert_eq!(stop_reason, StopReason::EndTurn);
    assert!(
        chunks.iter().any(|chunk| chunk == "hello"),
        "session/load must replay the user turn; got: {chunks:?}"
    );
    assert!(
        chunks.iter().any(|chunk| chunk == "hi there"),
        "session/load must replay the assistant turn; got: {chunks:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// session/list
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_integration_session_list_returns_sessions() -> Result<()> {
    // -- Setup & Fixtures
    let llm = start_llm_server().await?;
    mount_text_response(&llm, "unused", None).await?;
    let env = TestEnv::new(&llm.uri(), "")?;
    let recorder = Arc::new(Recorder::default());

    // -- Exec
    let (first, second, listed) = build_client(recorder)
        .connect_with(env.agent(), async |cx| {
            cx.send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;
            let first = cx
                .build_session(env.cwd())
                .block_task()
                .start_session()
                .await?;
            let first_id = first.session_id().to_string();
            let second = cx
                .build_session(env.cwd())
                .block_task()
                .start_session()
                .await?;
            let second_id = second.session_id().to_string();
            let listed = cx
                .send_request(ListSessionsRequest::new())
                .block_task()
                .await?;
            Ok((first_id, second_id, listed))
        })
        .await?;

    // -- Check
    let ids: Vec<String> = listed
        .sessions
        .iter()
        .map(|info| info.session_id.to_string())
        .collect();
    assert!(
        ids.contains(&first),
        "session/list must include the first session; got: {ids:?}"
    );
    assert!(
        ids.contains(&second),
        "session/list must include the second session; got: {ids:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// session/delete
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_integration_session_delete_removes_session() -> Result<()> {
    // -- Setup & Fixtures
    let llm = start_llm_server().await?;
    mount_text_response(&llm, "unused", None).await?;
    let env = TestEnv::new(&llm.uri(), "")?;
    let recorder = Arc::new(Recorder::default());

    // -- Exec
    let (session_id, listed) = build_client(recorder)
        .connect_with(env.agent(), async |cx| {
            cx.send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;
            let session = cx
                .build_session(env.cwd())
                .block_task()
                .start_session()
                .await?;
            let session_id = session.session_id().to_string();
            cx.send_request(DeleteSessionRequest::new(session_id.clone()))
                .block_task()
                .await?;
            let listed = cx
                .send_request(ListSessionsRequest::new())
                .block_task()
                .await?;
            Ok((session_id, listed))
        })
        .await?;

    // -- Check
    let ids: Vec<String> = listed
        .sessions
        .iter()
        .map(|info| info.session_id.to_string())
        .collect();
    assert!(
        !ids.contains(&session_id),
        "a deleted session must not be listed; got: {ids:?}"
    );
    Ok(())
}

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

// ---------------------------------------------------------------------------
// session/new config options
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_integration_new_session_advertises_model_config_option() -> Result<()> {
    // -- Setup & Fixtures
    let llm = start_llm_server().await?;
    mount_text_response(&llm, "unused", None).await?;
    let env = TestEnv::new(&llm.uri(), "")?;
    let recorder = Arc::new(Recorder::default());

    // -- Exec
    let options = build_client(recorder)
        .connect_with(env.agent(), async |cx| {
            cx.send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;
            let session = cx
                .build_session(env.cwd())
                .block_task()
                .start_session()
                .await?;
            session
                .config_options()
                .map(|options| options.to_vec())
                .ok_or_else(|| acp_err("session/new must advertise config options"))
        })
        .await?;

    // -- Check
    let model = options
        .iter()
        .find(|option| option.category == Some(SessionConfigOptionCategory::Model))
        .ok_or("a model config option must be advertised")?;
    let SessionConfigKind::Select(select) = &model.kind else {
        return Err("the model config option must be a select".into());
    };
    assert_eq!(
        select.current_value.to_string(),
        "ollama/llama3",
        "the advertised model must match the resolved config"
    );
    Ok(())
}
