use super::*;

/// The clap dump an MCP server writes when its command line is wrong: three
/// stacked blocks that a status row must never inherit.
const CLAP_STARTUP_DUMP: &str = "error: unexpected argument '--add-paths' found\n\n  tip: a similar argument exists: \
     '--add-path'\n\nUsage: nu-mcp --add-path <ADD_PATHS>";

/// A state whose one MCP server is failed with `reason` as its stored reason.
fn failing_mcp_state(reason: &str) -> Result<AppState> {
    let mut state = AppState::default();
    state
        .status
        .mcp
        .set_mcp_servers(vec![crate::state::McpServerState {
            name: "nu".to_string(),
            state: McpServerUsabilityState::Failed,
        }]);
    assert!(state.status.mcp.set_mcp_server_state_by_name_with_reason(
        "nu",
        McpServerUsabilityState::Failed,
        Some(reason.to_string())
    ));
    Ok(state)
}

/// The status band's failure line from `state`.
fn failure_line(state: &mut AppState) -> Result<String> {
    let line = crate::runtime::status_lines_for_test(state, "openai/gpt-4o-mini")
        .into_iter()
        .find(|line| line.starts_with("Failures: "))
        .ok_or_else(|| "the status content must carry a failure line".to_string())?;
    Ok(line)
}

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

/// The failure line is ONE terminal row: the multi-line clap dump collapses to
/// a single control-free line before the width clip.
#[test]
fn test_status_mcp_failure_line_single_lines_a_multiline_reason() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = failing_mcp_state(CLAP_STARTUP_DUMP)?;

    // -- Exec
    let line = failure_line(&mut state)?;

    // -- Check
    assert!(
        !crate::runtime::status::test::carries_control_byte(&line),
        "the failure line must carry no control byte, got: {line:?}"
    );
    // The line breaks become spaces, so the words stay apart and the 48-cell
    // reason budget stops inside the first block.
    assert_eq!(
        line, "Failures: nu (error: unexpected argument '--add-paths' found …)",
        "the failure line must normalize the reason to one clipped row"
    );
    Ok(())
}

/// Every status content line is control-free with the multi-line reason in
/// state: the band paints one content line per terminal row.
#[test]
fn test_status_build_status_lines_control_free_with_multiline_reason() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = failing_mcp_state(CLAP_STARTUP_DUMP)?;

    // -- Exec
    let lines = crate::runtime::status_lines_for_test(&mut state, "openai/gpt-4o-mini");

    // -- Check
    assert!(
        !lines.is_empty(),
        "the status content must not be empty, got: {lines:?}"
    );
    for line in &lines {
        assert!(
            !crate::runtime::status::test::carries_control_byte(line),
            "every status content line must be control-free, got: {line:?}"
        );
    }
    Ok(())
}

/// A single-line reason renders exactly as it did before the control-byte work.
#[test]
fn test_status_mcp_failure_line_keeps_single_line_reason_output() -> Result<()> {
    // -- Setup & Fixtures
    let mut short_state = failing_mcp_state("timeout")?;
    let long_reason = "x".repeat(60);
    let mut long_state = failing_mcp_state(&long_reason)?;

    // -- Exec
    let short = failure_line(&mut short_state)?;
    let long = failure_line(&mut long_state)?;

    // -- Check
    assert_eq!(short, "Failures: nu (timeout)");
    assert_eq!(long, format!("Failures: nu ({}…)", "x".repeat(47)));
    Ok(())
}
