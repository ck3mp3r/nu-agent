use super::*;

#[tokio::test]
async fn switch_model_send_failure_warns() {
    let mut h = Harness::new();
    worker_rx_drop(&mut h);
    let p = h.parts();
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::SwitchModel {
                spec: "test-model".to_string(),
            },
            &mut ctx,
        )
        .await;

    let warnings = take_warnings(p.narration_rx);
    assert!(
        warnings
            .iter()
            .any(|w| w == "Model switch worker channel closed")
    );
    assert!(!p.stage.has_blocking_pending());
}

// ---------------------------------------------------------------------------
// handle_blocking_response tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn blocking_response_model_switch_success() {
    let mut h = Harness::new();
    let p = h.parts();
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::SwitchModel {
                spec: "test-model".to_string(),
            },
            &mut ctx,
        )
        .await;
    let _ = recv_command(p.worker_rx);

    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_blocking_response(
            UiRequestResponse::ModelSwitch(Ok(("test-model".to_string(), Some(128000)))),
            &mut ctx,
        )
        .await;

    let facts = take_facts(p.facts_rx);
    assert!(
        facts
            .iter()
            .any(|f| matches!(f, FactRecord::ActiveModelIdentity(id) if id == "test-model"))
    );
    assert!(
        facts
            .iter()
            .any(|f| matches!(f, FactRecord::ContextWindowMaxTokens(Some(128000))))
    );
    assert!(!p.stage.has_blocking_pending());
}

#[tokio::test]
async fn blocking_response_model_switch_error() {
    let mut h = Harness::new();
    let p = h.parts();
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::SwitchModel {
                spec: "test-model".to_string(),
            },
            &mut ctx,
        )
        .await;
    let _ = recv_command(p.worker_rx);

    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_blocking_response(
            UiRequestResponse::ModelSwitch(Err("model not found".to_string())),
            &mut ctx,
        )
        .await;

    let warnings = take_warnings(p.narration_rx);
    assert!(warnings.iter().any(|w| w == "model not found"));
    assert!(!p.stage.has_blocking_pending());
}

#[tokio::test]
async fn blocking_response_agent_switch_success() {
    let mut h = Harness::new();
    let p = h.parts();
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::SwitchAgent {
                name: "research-agent".to_string(),
            },
            &mut ctx,
        )
        .await;
    let _ = recv_command(p.worker_rx);

    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_blocking_response(
            UiRequestResponse::AgentSwitch(Ok((
                "research-agent".to_string(),
                "openai/gpt-4o".to_string(),
                Some(200000),
                Some("icon".to_string()),
            ))),
            &mut ctx,
        )
        .await;

    let facts = take_facts(p.facts_rx);
    assert!(
        facts
            .iter()
            .any(|f| matches!(f, FactRecord::ActiveAgentIdentity(id) if id == "research-agent"))
    );
    assert!(
        facts
            .iter()
            .any(|f| matches!(f, FactRecord::ActivePersonaIcon(Some(icon)) if icon == "icon"))
    );
    assert!(
        facts
            .iter()
            .any(|f| matches!(f, FactRecord::ActiveModelIdentity(id) if id == "openai/gpt-4o"))
    );
    assert!(
        facts
            .iter()
            .any(|f| matches!(f, FactRecord::ContextWindowMaxTokens(Some(200000))))
    );
    assert!(!p.stage.has_blocking_pending());
}

#[tokio::test]
async fn blocking_response_agent_switch_error() {
    let mut h = Harness::new();
    let p = h.parts();
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::SwitchAgent {
                name: "research-agent".to_string(),
            },
            &mut ctx,
        )
        .await;
    let _ = recv_command(p.worker_rx);

    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_blocking_response(
            UiRequestResponse::AgentSwitch(Err("agent not found".to_string())),
            &mut ctx,
        )
        .await;

    let warnings = take_warnings(p.narration_rx);
    assert!(warnings.iter().any(|w| w == "agent not found"));
    assert!(!p.stage.has_blocking_pending());
}

#[tokio::test]
async fn blocking_response_session_switch_success() {
    let mut h = Harness::new();
    let p = h.parts();
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::SwitchSession {
                id: "session-1".to_string(),
            },
            &mut ctx,
        )
        .await;
    let _ = recv_command(p.worker_rx);

    let snapshot = UiMessageSnapshot::new("user", "hello");
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_blocking_response(
            UiRequestResponse::SessionSwitch {
                id: "session-1".to_string(),
                result: Ok(vec![snapshot]),
            },
            &mut ctx,
        )
        .await;

    // The switch marker and the replayed rows ride the stream: the surface
    // replaces its transcript on the hydrated marker, so there is no separate
    // clear or logo event to observe.
    let records = take_records(p.stream_rx);
    assert!(
        records
            .iter()
            .any(|r| matches!(r, SessionRecord::Session(SessionLifecycle::Switched { .. })))
    );
    assert_eq!(
        records
            .iter()
            .filter(|r| matches!(r, SessionRecord::Prompt(_)))
            .count(),
        1,
        "the replay carries the one snapshot, got {records:?}"
    );
    let lifecycle = take_lifecycle(p.lifecycle_rx);
    assert!(
        lifecycle
            .iter()
            .any(|e| matches!(e, LifecycleEvent::SessionSwitched { to, .. } if to == "session-1"))
    );
    assert!(!p.stage.has_blocking_pending());
}

#[tokio::test]
async fn blocking_response_session_switch_error() {
    let mut h = Harness::new();
    let p = h.parts();
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::SwitchSession {
                id: "session-1".to_string(),
            },
            &mut ctx,
        )
        .await;
    let _ = recv_command(p.worker_rx);

    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_blocking_response(
            UiRequestResponse::SessionSwitch {
                id: "session-1".to_string(),
                result: Err("session not found".to_string()),
            },
            &mut ctx,
        )
        .await;

    let warnings = take_warnings(p.narration_rx);
    assert!(warnings.iter().any(|w| w == "session not found"));
    assert!(!p.stage.has_blocking_pending());
}
