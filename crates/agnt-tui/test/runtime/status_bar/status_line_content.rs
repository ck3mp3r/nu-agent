use super::*;

/// The status lines expose the stable model identity and MCP backend counts,
/// including the failed count when present, and the panel renders them.
#[test]
fn status_lines_model_and_mcp_content() -> Result<()> {
    // -- Exec & Check: the stable active model identity line.
    let mut state = AppState::default();
    state.status.mcp.set_mcp_servers(vec![
        crate::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        },
        crate::state::McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Disabled,
        },
    ]);
    let status_lines = crate::runtime::status_lines_for_test(&mut state, "openai/gpt-4o-mini");
    assert!(
        status_lines
            .iter()
            .any(|line| line == "Model: openai/gpt-4o-mini (idle)")
    );
    assert!(
        status_lines
            .iter()
            .any(|line| line == "MCP: configured=2 enabled=1 disabled=1 failed=0")
    );
    assert!(!status_lines.iter().any(|line| line.starts_with("Hint:")));
    assert!(
        !status_lines
            .iter()
            .any(|line| line.starts_with("Input backend:"))
    );
    assert!(
        !status_lines
            .iter()
            .any(|line| line.starts_with("Input poll:"))
    );
    assert!(
        !status_lines
            .iter()
            .any(|line| line.starts_with("Input error:"))
    );

    // -- Exec & Check: the panel exposes the model and MCP backend lines.
    let mut panel = AppState::default();
    panel.status.identity.active_model_identity = "openai/gpt-4o-mini".to_string();
    panel.status.mcp.set_mcp_servers(vec![
        crate::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        },
        crate::state::McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Disabled,
        },
    ]);
    let (title, lines) = status_panel_lines(&panel);
    assert_eq!(title, "Status");
    let rendered = lines
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("Model: openai/gpt-4o-mini (idle)"));
    assert!(rendered.contains("MCP: configured=2 enabled=1 disabled=1 failed=0"));
    assert!(rendered.contains("LLM-visible MCP tools: 0"));
    assert!(rendered.contains("Failures: none (healthy)"));
    assert!(!rendered.contains("Hint: Ctrl-P -> MCPs"));
    assert!(!rendered.contains("Input backend:"));
    assert!(!rendered.contains("Input poll:"));
    assert!(!rendered.contains("Input error:"));
    assert!(!rendered.contains("MCP + Model status"));
    assert!(!rendered.contains("MCP/Input backend:"));

    // -- Exec & Check: the failed count is reported when present.
    let mut failed = AppState::default();
    failed.status.mcp.set_mcp_servers(vec![
        crate::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        },
        crate::state::McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Failed,
        },
    ]);
    let status_lines = crate::runtime::status_lines_for_test(&mut failed, "openai/gpt-4o-mini");
    assert!(
        status_lines
            .iter()
            .any(|line| line == "MCP: configured=2 enabled=1 disabled=0 failed=1")
    );
    Ok(())
}

/// The status lines report the LLM-visible MCP tool count before any LLM end
/// and keep the model line after LLM end events.
#[test]
fn status_lines_tokens_and_llm_end() -> Result<()> {
    // -- Exec & Check: the visible tool count is zero before any LLM end.
    let mut state = AppState::default();
    let status_lines = crate::runtime::status_lines_for_test(&mut state, "openai/gpt-4o-mini");
    assert!(
        status_lines
            .iter()
            .any(|line| line == "LLM-visible MCP tools: 0")
    );

    // -- Setup & Fixtures: a coordinator with two LLM completions.
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.enqueue_ui_event(UiEvent::LlmCompleted {
        response_chars: 10,
        tool_calls: 0,
        input_tokens: 11,
        output_tokens: 9,
        total_tokens: 20,
    });
    coordinator.drain_transport();
    coordinator.enqueue_ui_event(UiEvent::LlmCompleted {
        response_chars: 12,
        tool_calls: 0,
        input_tokens: 3,
        output_tokens: 4,
        total_tokens: 7,
    });
    coordinator.drain_transport();

    // -- Check
    let status_lines =
        crate::runtime::status_lines_for_test(&mut coordinator.state, "openai/gpt-4o-mini");
    assert!(
        status_lines
            .iter()
            .any(|line| line == "Model: openai/gpt-4o-mini (idle)")
    );
    assert!(
        status_lines
            .iter()
            .any(|line| line == "LLM-visible MCP tools: 0")
    );
    Ok(())
}
