use super::*;

// ---------------------------------------------------------------------------
// handle_concurrent_response tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn concurrent_response_mcp_toggle_success() {
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
            UiRequest::ToggleMcp {
                server: "test-server".to_string(),
                enable: true,
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
        .handle_concurrent_response(
            UiRequestResponse::McpToggle {
                server: "test-server".to_string(),
                result: Ok(McpUsabilityState::Enabled),
                total: 5,
                server_count: 3,
                names_by_server: vec![("test-server".to_string(), vec!["tool1".to_string()])],
            },
            &mut ctx,
        )
        .await;

    let facts = take_facts(p.facts_rx);
    assert!(facts.iter().any(|f| matches!(f, FactRecord::McpServerState { server, state, error } if server == "test-server" && *state == McpServerUsability::Enabled && error.is_none())));
    assert!(facts.iter().any(|f| matches!(f, FactRecord::McpVisibleToolCount { server, count } if server == "test-server" && *count == 3)));
    assert!(facts.iter().any(|f| matches!(f, FactRecord::McpVisibleToolNames { server, names } if server == "test-server" && names == &vec!["tool1".to_string()])));
    assert!(
        facts
            .iter()
            .any(|f| matches!(f, FactRecord::McpVisibleToolTotal(5)))
    );
    assert!(!p.stage.has_pending());
}

#[tokio::test]
async fn concurrent_response_mcp_toggle_error() {
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
            UiRequest::ToggleMcp {
                server: "test-server".to_string(),
                enable: true,
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
        .handle_concurrent_response(
            UiRequestResponse::McpToggle {
                server: "test-server".to_string(),
                result: Err("toggle failed".to_string()),
                total: 5,
                server_count: 3,
                names_by_server: Vec::new(),
            },
            &mut ctx,
        )
        .await;

    let facts = take_facts(p.facts_rx);
    assert!(facts.iter().any(|f| matches!(f, FactRecord::McpServerState { server, state, error } if server == "test-server" && *state == McpServerUsability::Failed && error.as_deref() == Some("toggle failed"))));
    assert!(
        facts
            .iter()
            .any(|f| matches!(f, FactRecord::McpVisibleToolTotal(5)))
    );
    assert!(!p.stage.has_pending());
}
