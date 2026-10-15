use super::*;

/// The status contract reports the model identity with its busy/idle state and
/// excludes input-mode, backend, poll, error, and hint lines.
#[test]
fn status_contract_model_line_and_exclusions() -> Result<()> {
    // -- Exec & Check: the model line reports identity and busy/idle.
    let mut state = AppState::default();
    let idle_lines = crate::runtime::status_lines_for_test(&mut state, "openai/gpt-4o-mini");
    assert!(
        idle_lines
            .iter()
            .any(|line| line == "Model: openai/gpt-4o-mini (idle)")
    );
    state.phase = UiPhase::Busy;
    let busy_lines = crate::runtime::status_lines_for_test(&mut state, "openai/gpt-4o-mini");
    assert!(
        busy_lines
            .iter()
            .any(|line| line == "Model: openai/gpt-4o-mini (busy)")
    );

    // -- Exec & Check: input-mode, backend, poll, error, and hint lines are excluded.
    let mut state = AppState::default();
    let lines = crate::runtime::status_lines_for_test(&mut state, "openai/gpt-4o-mini");
    assert!(!lines.iter().any(|line| line.starts_with("Input mode:")));
    assert!(!lines.iter().any(|line| line.starts_with("Input backend:")));
    assert!(!lines.iter().any(|line| line.starts_with("Input poll:")));
    assert!(!lines.iter().any(|line| line.starts_with("Input error:")));
    assert!(!lines.iter().any(|line| line.starts_with("Hint:")));
    Ok(())
}

/// The status contract reports MCP counts, the LLM-visible tool count, failure
/// names and reasons, and a compact deterministic narrow layout.
#[test]
fn status_contract_mcp_counts_failures_and_narrow_layout() -> Result<()> {
    // -- Exec & Check: configured/enabled/disabled/failed counts.
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
        crate::state::McpServerState {
            name: "docs".to_string(),
            state: McpServerUsabilityState::Failed,
        },
    ]);
    let lines = crate::runtime::status_lines_for_test(&mut state, "openai/gpt-4o-mini");
    assert!(
        lines
            .iter()
            .any(|line| line == "MCP: configured=3 enabled=1 disabled=1 failed=1")
    );

    // -- Exec & Check: the LLM-visible tool count uses runtime truth and updates.
    let mut visible = AppState::default();
    visible.status.mcp.set_llm_visible_mcp_tool_count(5);
    let before = crate::runtime::status_lines_for_test(&mut visible, "openai/gpt-4o-mini");
    assert!(before.iter().any(|line| line == "LLM-visible MCP tools: 5"));
    visible.status.mcp.set_llm_visible_mcp_tool_count(2);
    let after = crate::runtime::status_lines_for_test(&mut visible, "openai/gpt-4o-mini");
    assert!(after.iter().any(|line| line == "LLM-visible MCP tools: 2"));

    // -- Exec & Check: failures show names and reasons; healthy shows none.
    let mut failed = AppState::default();
    failed.status.mcp.set_mcp_servers(vec![
        crate::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Failed,
        },
        crate::state::McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Failed,
        },
    ]);
    assert!(failed.status.mcp.set_mcp_server_state_by_name_with_reason(
        "gh",
        McpServerUsabilityState::Failed,
        Some("timeout".to_string())
    ));
    assert!(failed.status.mcp.set_mcp_server_state_by_name_with_reason(
        "k8s",
        McpServerUsabilityState::Failed,
        None
    ));
    let failed_lines = crate::runtime::status_lines_for_test(&mut failed, "openai/gpt-4o-mini");
    assert!(
        failed_lines
            .join("\n")
            .contains("Failures: gh (timeout), k8s")
    );
    assert!(failed.status.mcp.set_mcp_server_state_by_name_with_reason(
        "gh",
        McpServerUsabilityState::Enabled,
        None
    ));
    assert!(failed.status.mcp.set_mcp_server_state_by_name_with_reason(
        "k8s",
        McpServerUsabilityState::Enabled,
        None
    ));
    let healthy_lines = crate::runtime::status_lines_for_test(&mut failed, "openai/gpt-4o-mini");
    assert!(
        healthy_lines
            .iter()
            .any(|line| line == "Failures: none (healthy)")
    );

    // -- Exec & Check: a narrow layout is compact and ellipsizes deterministically.
    let mut narrow = AppState {
        phase: UiPhase::Busy,
        ..Default::default()
    };
    narrow
        .status
        .mcp
        .set_mcp_servers(vec![crate::state::McpServerState {
            name: "very-long-mcp-server-name-that-must-be-truncated".to_string(),
            state: McpServerUsabilityState::Failed,
        }]);
    assert!(narrow.status.mcp.set_mcp_server_state_by_name_with_reason(
        "very-long-mcp-server-name-that-must-be-truncated",
        McpServerUsabilityState::Failed,
        Some("very long failure reason that should be truncated to keep the status line readable"
            .to_string())
    ));
    narrow.status.mcp.set_llm_visible_mcp_tool_count(42);
    let lines = crate::runtime::status_lines_for_test(
        &mut narrow,
        "provider/super-long-model-name-that-needs-truncation",
    );
    let rendered = lines.join("\n");
    assert!(rendered.contains('…'));
    assert!(!rendered.contains("Hint: Ctrl-P -> MCPs"));
    let compact = crate::runtime::compact_status_line_for_test(
        "provider/super-long-model-name-that-needs-truncation",
        None,
    );
    let compact_text: String = compact.spans.iter().map(|s| s.content.as_ref()).collect();
    let compact_narrow = crate::runtime::status::test::compact_status_line_with_branch_for_test(
        "provider/super-long-model-name-that-needs-truncation",
        Some("feature/very-long-branch-name-that-needs-truncation"),
        None,
        24,
    );
    let compact_narrow_text: String = compact_narrow
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    assert!(!compact_text.starts_with("❯ "));
    assert!(!compact_text.contains('|'));
    assert!(compact_narrow_text.contains("..."));
    assert!(!compact_narrow_text.contains('|'));
    Ok(())
}
