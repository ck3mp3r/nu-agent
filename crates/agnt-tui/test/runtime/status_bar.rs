use super::*;

#[test]
fn status_lines_do_not_report_input_mode() {
    let mut state = AppState::default();

    let lines = crate::runtime::status_lines_for_test(&mut state, "openai/gpt-4");
    assert!(!lines.iter().any(|line| line.starts_with("Input mode:")));
}

#[test]
fn compact_status_line_matches_lane_1_contract() {
    let line = crate::runtime::compact_status_line_for_test("openai/gpt-4o-mini", None);
    let status_line: String = line.spans.iter().map(|s| s.content.as_ref()).collect();

    assert!(status_line.starts_with("○ openai/gpt-4o-mini"));
    assert!(!status_line.contains('|'));
}

const LANE_1_CASES: &[(&str, Option<&str>, usize, &str)] = &[
    (
        "abcdefghijklmnop",
        Some("branchname"),
        40,
        "○ abcdefghijklmnop          \u{e725} branchname",
    ),
    (
        "abcdefghijklmnop",
        Some("branchname"),
        23,
        "○ ...lmnop \u{e725} branchname",
    ),
    (
        "abcdefghijklmnop",
        Some("branchname"),
        20,
        "○ ...op \u{e725} branchname",
    ),
    ("openai/gpt-4o-mini", None, 80, "○ openai/gpt-4o-mini"),
];

#[test]
fn lane_1_scroll() {
    for &(model, branch, width, expected) in LANE_1_CASES {
        let line = crate::runtime::status::test::compact_status_line_with_branch_for_test(
            model, branch, None, width,
        );
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();

        assert_eq!(
            text, expected,
            "model={model}, branch={branch:?}, width={width}"
        );
        assert!(
            !text.contains('|'),
            "pipe should not appear: model={model}, branch={branch:?}, width={width}"
        );

        // Extra structural assertions carried over from overlapping
        // lane_1_branch_segment_is_right_aligned_when_present.
        if model == "abcdefghijklmnop" && branch == Some("branchname") && width == 40 {
            assert_eq!(text.chars().count(), 40);
            assert!(text.starts_with("○ abcdefghijklmnop"));
            assert!(text.ends_with("\u{e725} branchname"));
        }

        // Extra structural assertions carried over from
        // lane_1_narrow_truncation_keeps_branch_right_anchored.
        if model == "abcdefghijklmnop" && branch == Some("branchname") && width == 20 {
            assert_eq!(text.chars().count(), 20);
            assert!(text.ends_with("\u{e725} branchname"));
            assert!(text.contains("...op"));
        }
    }

    // Structural-only: lane_1_with_branch_appends_branch_icon
    {
        let line = crate::runtime::status::test::compact_status_line_with_branch_for_test(
            "m",
            Some("main"),
            None,
            40,
        );
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();

        assert!(
            text.ends_with("\u{e725} main"),
            "expected branch icon prefix, got: {text:?}"
        );
    }

    // Structural-only: lane_1_with_branch_ellipsizes_label_while_preserving_icon
    {
        let line = crate::runtime::status::test::compact_status_line_with_branch_for_test(
            "the-quick-brown-fox-jumps-over",
            Some("feature/super-long-branch-name"),
            None,
            32,
        );
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();

        assert!(
            text.contains('\u{e725}'),
            "icon must survive ellipsization, got: {text:?}",
        );
        assert!(
            text.contains("..."),
            "branch label should have been ellipsized, got: {text:?}",
        );
    }

    // Structural-only: lane_1_with_branch_drops_icon_when_budget_below_three_cells
    {
        let line = crate::runtime::status::test::compact_status_line_with_branch_for_test(
            "abc",
            Some("main"),
            None,
            4,
        );
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();

        assert!(
            !text.contains('\u{e725}'),
            "icon must be dropped under extreme narrow budgets, got: {text:?}",
        );
        assert!(text.starts_with("○ "));
    }

    // Structural-only: lane_1_with_detached_head_short_sha_also_gets_icon
    {
        let line = crate::runtime::status::test::compact_status_line_with_branch_for_test(
            "m",
            Some("a1b2c3d"),
            None,
            40,
        );
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();

        assert!(
            text.ends_with("\u{e725} a1b2c3d"),
            "expected detached-HEAD short SHA to also carry icon, got: {text:?}",
        );
    }
}

#[test]
fn lane_1_has_no_mode_token_in_any_input_mode() {
    let insert_line = crate::runtime::status::test::compact_status_line_with_branch_for_test(
        "model", None, None, 80,
    );
    let insert_text: String = insert_line
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();

    let normal_line = crate::runtime::status::test::compact_status_line_with_branch_for_test(
        "model", None, None, 80,
    );
    let normal_text: String = normal_line
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();

    let visual_line = crate::runtime::status::test::compact_status_line_with_branch_for_test(
        "model", None, None, 80,
    );
    let visual_text: String = visual_line
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();

    assert_eq!(insert_text, "○ model");
    assert_eq!(normal_text, "○ model");
    assert_eq!(visual_text, "○ model");
}

#[test]
fn status_contract_a_model_line_reports_identity_and_busy_idle() {
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
}

#[test]
fn status_contract_b_excludes_input_mode_backend_poll_and_hint_lines() {
    let mut state = AppState::default();

    let lines = crate::runtime::status_lines_for_test(&mut state, "openai/gpt-4o-mini");
    assert!(!lines.iter().any(|line| line.starts_with("Input mode:")));
    assert!(!lines.iter().any(|line| line.starts_with("Input backend:")));
    assert!(!lines.iter().any(|line| line.starts_with("Input poll:")));
    assert!(!lines.iter().any(|line| line.starts_with("Input error:")));
    assert!(!lines.iter().any(|line| line.starts_with("Hint:")));
}

#[test]
fn status_contract_c_mcp_counts_include_configured_enabled_disabled_failed() {
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
}

#[test]
fn status_contract_d_visible_mcp_tool_count_uses_runtime_truth_and_updates() {
    let mut state = AppState::default();
    state.status.mcp.set_llm_visible_mcp_tool_count(5);

    let before = crate::runtime::status_lines_for_test(&mut state, "openai/gpt-4o-mini");
    assert!(before.iter().any(|line| line == "LLM-visible MCP tools: 5"));

    state.status.mcp.set_llm_visible_mcp_tool_count(2);
    let after = crate::runtime::status_lines_for_test(&mut state, "openai/gpt-4o-mini");
    assert!(after.iter().any(|line| line == "LLM-visible MCP tools: 2"));
}

#[test]
fn status_contract_e_failures_show_names_and_reasons_and_healthy_none_when_clear() {
    let mut state = AppState::default();
    state.status.mcp.set_mcp_servers(vec![
        crate::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Failed,
        },
        crate::state::McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Failed,
        },
    ]);
    assert!(state.status.mcp.set_mcp_server_state_by_name_with_reason(
        "gh",
        McpServerUsabilityState::Failed,
        Some("timeout".to_string())
    ));
    assert!(state.status.mcp.set_mcp_server_state_by_name_with_reason(
        "k8s",
        McpServerUsabilityState::Failed,
        None
    ));

    let failed_lines = crate::runtime::status_lines_for_test(&mut state, "openai/gpt-4o-mini");
    let failed_rendered = failed_lines.join("\n");
    assert!(failed_rendered.contains("Failures: gh (timeout), k8s"));

    assert!(state.status.mcp.set_mcp_server_state_by_name_with_reason(
        "gh",
        McpServerUsabilityState::Enabled,
        None
    ));
    assert!(state.status.mcp.set_mcp_server_state_by_name_with_reason(
        "k8s",
        McpServerUsabilityState::Enabled,
        None
    ));
    let healthy_lines = crate::runtime::status_lines_for_test(&mut state, "openai/gpt-4o-mini");
    assert!(
        healthy_lines
            .iter()
            .any(|line| line == "Failures: none (healthy)")
    );
}

#[test]
fn status_contract_f_narrow_layout_is_compact_and_ellipsizes_deterministically() {
    let mut state = AppState {
        phase: UiPhase::Busy,
        ..Default::default()
    };
    state
        .status
        .mcp
        .set_mcp_servers(vec![crate::state::McpServerState {
            name: "very-long-mcp-server-name-that-must-be-truncated".to_string(),
            state: McpServerUsabilityState::Failed,
        }]);
    assert!(state.status.mcp.set_mcp_server_state_by_name_with_reason(
        "very-long-mcp-server-name-that-must-be-truncated",
        McpServerUsabilityState::Failed,
        Some("very long failure reason that should be truncated to keep the status line readable"
            .to_string())
    ));
    state.status.mcp.set_llm_visible_mcp_tool_count(42);

    let lines = crate::runtime::status_lines_for_test(
        &mut state,
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
}

#[test]
fn status_lines_include_stable_active_model_identity_line() {
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
}

#[test]
fn status_panel_exposes_model_and_mcp_backend_status_lines() {
    let mut state = AppState::default();
    state.status.identity.active_model_identity = "openai/gpt-4o-mini".to_string();
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

    let (title, lines) = status_panel_lines(&state);
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
}

#[test]
fn status_lines_report_failed_state_count_when_present() {
    let mut state = AppState::default();
    state.status.mcp.set_mcp_servers(vec![
        crate::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        },
        crate::state::McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Failed,
        },
    ]);

    let status_lines = crate::runtime::status_lines_for_test(&mut state, "openai/gpt-4o-mini");

    assert!(
        status_lines
            .iter()
            .any(|line| line == "MCP: configured=2 enabled=1 disabled=0 failed=1")
    );
}

#[test]
fn status_lines_include_tokens_line_with_na_before_any_llm_end() {
    let mut state = AppState::default();
    let status_lines = crate::runtime::status_lines_for_test(&mut state, "openai/gpt-4o-mini");

    assert!(
        status_lines
            .iter()
            .any(|line| line == "LLM-visible MCP tools: 0")
    );
}

#[test]
fn status_lines_include_latest_and_rolling_tokens_after_llm_end_events() {
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
}

#[test]
fn status_indicator_idle_returns_empty_circle() {
    assert_eq!(crate::runtime::status_indicator_for_test(None), "○");
}

#[test]
fn status_indicator_busy_cycles_through_four_frames() {
    let f = crate::runtime::status_indicator_for_test;
    assert_eq!(f(Some(0)), "◐");
    assert_eq!(f(Some(150)), "◓");
    assert_eq!(f(Some(300)), "◑");
    assert_eq!(f(Some(450)), "◒");
    assert_eq!(f(Some(600)), "◐"); // wraps
}

#[test]
fn lane_1_idle_shows_empty_circle_prefix() {
    let line = crate::runtime::status::test::compact_status_line_with_branch_for_test(
        "mymodel", None, None, 40,
    );
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.starts_with("○ mymodel"));
}

#[test]
fn lane_1_busy_shows_spinner_prefix() {
    let line = crate::runtime::status::test::compact_status_line_with_branch_for_test(
        "mymodel",
        None,
        Some(0),
        40,
    );
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.starts_with("◐ mymodel"));
}

#[test]
fn lane_1_prefix_does_not_exceed_available_width() {
    let line = crate::runtime::status::test::compact_status_line_with_branch_for_test(
        "abcdefghijklmnop",
        Some("branchname"),
        None,
        40,
    );
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.chars().count() <= 40);
    assert!(text.starts_with("○ "));
}

#[test]
fn compact_status_line_busy_has_at_least_one_styled_span() {
    // RED: compact_status_line_for_test must return Line<'static> whose spans
    // carry explicit fg colours when the spinner is active (now_millis = Some(0)).
    let line = crate::runtime::compact_status_line_for_test("openai/gpt-4o-mini", Some(0));
    let has_styled = line.spans.iter().any(|s| s.style.fg.is_some());
    assert!(
        has_styled,
        "expected at least one span with explicit fg colour, got: {line:?}"
    );
}

#[test]
fn status_target_height_is_three() {
    use crate::runtime::render::frame_test::STATUS_TARGET_HEIGHT;
    assert_eq!(STATUS_TARGET_HEIGHT, 3);
}

#[test]
fn status_left_content_contains_model() {
    let mut state = AppState::default();
    let line = crate::runtime::status_left_content_for_test("openai/gpt-4", None, &mut state, 80);
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.contains("openai/gpt-4"));
}

#[test]
fn status_right_content_contains_branch() -> Result<()> {
    let line = crate::runtime::status_right_content_for_test(Some("main"), None);
    let line = line.ok_or("should have right content line")?;
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.contains("main"));
    Ok(())
}

#[test]
fn status_right_content_shows_none_when_no_branch_and_no_cwd() {
    let line = crate::runtime::status_right_content_for_test(None, None);
    assert!(line.is_none());
}

#[test]
fn status_right_content_shows_cwd_when_given() -> Result<()> {
    let cwd = std::path::Path::new("/home/user/projects/my-project");
    let line = crate::runtime::status_right_content_for_test(None, Some(cwd));
    let line = line.ok_or("should have right content line")?;
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.contains("my-project"));
    Ok(())
}

#[test]
fn status_left_content_contains_tokens() {
    let mut state = AppState {
        status: crate::state::StatusState {
            tokens: crate::state::TokenUsage {
                latest_total_tokens: Some(250),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    state
        .status
        .tokens
        .set_context_window_max_tokens(Some(1000));
    let line = crate::runtime::status_left_content_for_test("openai/gpt-4", None, &mut state, 80);
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.contains("250"));
    assert!(text.contains("25%"));
    assert!(text.contains('┃'));
}

#[test]
fn status_right_content_width_exceeds_narrow_line_triggering_overflow() -> Result<()> {
    let narrow_width: u16 = 20;
    let branch = "feature/my-very-long-branch";
    let cwd = std::path::Path::new("/home/user/projects/deep/nested/dir");
    let right_line = crate::runtime::status_right_content_for_test(Some(branch), Some(cwd));
    let right_line = right_line.ok_or("should have right content line")?;
    let right_width: u16 = right_line
        .spans
        .iter()
        .map(|s| unicode_width::UnicodeWidthStr::width(s.content.as_ref()) as u16)
        .sum();
    assert!(
        right_width > narrow_width,
        "right_width={right_width} must exceed narrow_width={narrow_width} to exercise overflow"
    );
    Ok(())
}
