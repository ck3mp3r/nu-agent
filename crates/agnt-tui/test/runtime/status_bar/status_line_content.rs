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
    let (title, lines) = status_panel_lines(&panel, 200);
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
    coordinator.enqueue_event(llm_completed(10, 0, 11, 9, Some(20), None, false));
    coordinator.drain_transport();
    coordinator.enqueue_event(llm_completed(12, 0, 3, 4, Some(7), None, false));
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

/// A priced completion shows the cumulative session cost next to the token
/// count and percentage.
#[test]
fn status_line_shows_cumulative_session_cost() -> Result<()> {
    // -- Setup & Fixtures: a known context window and two priced calls.
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator
        .state
        .status
        .tokens
        .set_context_window_max_tokens(Some(128_000));
    coordinator.enqueue_event(llm_completed(
        40,
        0,
        2_500,
        500,
        Some(3_000),
        Some(0.001),
        true,
    ));
    coordinator.drain_transport();
    coordinator.enqueue_event(llm_completed(
        40,
        0,
        2_500,
        500,
        Some(3_000),
        Some(0.002),
        true,
    ));
    coordinator.drain_transport();

    // -- Exec
    let line = crate::runtime::status_left_content_for_test(
        "openai/gpt-4.1",
        None,
        &mut coordinator.state,
        120,
    );

    // -- Check: the latest token count and percentage, then the summed cost.
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.contains("3k (2%) · $0.003"), "text={text}");
    Ok(())
}

/// An unpriced completion shows the token count and percentage alone, with no
/// `$0.000` placeholder.
#[test]
fn status_line_shows_tokens_only_when_unpriced() -> Result<()> {
    // -- Setup & Fixtures: a known context window and an unpriced call.
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator
        .state
        .status
        .tokens
        .set_context_window_max_tokens(Some(128_000));
    coordinator.enqueue_event(llm_completed(40, 0, 2_500, 500, Some(3_000), None, false));
    coordinator.drain_transport();

    // -- Exec
    let line = crate::runtime::status_left_content_for_test(
        "github-copilot/gpt-4.1",
        None,
        &mut coordinator.state,
        120,
    );

    // -- Check
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(!text.contains('$'), "text={text}");
    assert!(text.contains("3k (2%)"), "text={text}");
    Ok(())
}

/// A session that mixes priced and unpriced calls keeps the priced total and
/// marks it a lower bound with the `+` suffix.
#[test]
fn status_line_marks_mixed_pricing_with_plus() -> Result<()> {
    // -- Setup & Fixtures: one priced call and one unpriced call.
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator
        .state
        .status
        .tokens
        .set_context_window_max_tokens(Some(128_000));
    coordinator.enqueue_event(llm_completed(
        40,
        0,
        2_500,
        500,
        Some(3_000),
        Some(0.001),
        true,
    ));
    coordinator.drain_transport();
    coordinator.enqueue_event(llm_completed(40, 0, 2_500, 500, Some(3_000), None, false));
    coordinator.drain_transport();

    // -- Exec
    let line = crate::runtime::status_left_content_for_test(
        "openai/gpt-4.1",
        None,
        &mut coordinator.state,
        120,
    );

    // -- Check
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.contains("3k (2%) · $0.001+"), "text={text}");
    Ok(())
}

/// With no known context window the status shows the cost and the token count
/// without a percentage.
#[test]
fn status_line_shows_cost_without_percentage_when_max_unknown() -> Result<()> {
    // -- Setup & Fixtures: a priced call and no context window.
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.enqueue_event(llm_completed(
        40,
        0,
        2_500,
        500,
        Some(3_000),
        Some(0.001),
        true,
    ));
    coordinator.drain_transport();

    // -- Exec
    let line = crate::runtime::status_left_content_for_test(
        "openai/gpt-4.1",
        None,
        &mut coordinator.state,
        120,
    );

    // -- Check
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.contains("3k · $0.001"), "text={text}");
    assert!(!text.contains('%'), "text={text}");
    Ok(())
}

/// A completion that reports no total leaves the hydrated token count alone.
/// An absent total is not a zero, so a resumed session's count must not
/// collapse to `0 (0%)` when a total-less wire answers.
#[test]
fn test_token_usage_total_less_completion_preserves_hydrated_count() -> Result<()> {
    // -- Setup & Fixtures: a resumed session's hydrated count and window.
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator
        .state
        .status
        .tokens
        .hydrate_latest_total_tokens(42_300);
    coordinator
        .state
        .status
        .tokens
        .set_context_window_max_tokens(Some(128_000));

    // -- Exec: a completion whose wire omitted the total.
    coordinator.enqueue_event(llm_completed(20, 0, 11, 9, None, None, false));
    coordinator.drain_transport();

    // -- Check: the hydrated count and percentage survive.
    let line = crate::runtime::status_left_content_for_test(
        "ollama-cloud/deepseek-v4.1-flash",
        None,
        &mut coordinator.state,
        120,
    );
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.contains("42.3k (33%)"), "text={text}");
    assert!(!text.contains("0 (0%)"), "text={text}");
    Ok(())
}
