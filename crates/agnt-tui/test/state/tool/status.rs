use super::*;

// ---------------------------------------------------------------------------
// Tool call lifecycle and status bookkeeping
// ---------------------------------------------------------------------------

#[test]
fn tool_end_transcript_line_shows_args_summary_without_result_payload_dump() {
    let mut state = AppState::default();
    reduce_tool(
        &mut state,
        started("k8s__list_pods", r#"{"namespace":"prod"}"#),
    );
    reduce_tool(
        &mut state,
        ToolEvent::Completed {
            name: "k8s__list_pods".to_string(),
            source: "mcp".to_string(),
            arguments: r#"{"namespace":"prod"}"#.to_string(),
            success: true,
            result: "[{\"name\":\"api-0\",\"ns\":\"prod\"}]".to_string(),
            display: None,
            error_kind: None,
            message: None,
        },
    );

    // [Tool] — no leading spacer under the unified spacer rule
    assert_eq!(state.transcript.len(), 1);
    let block = &state.transcript.blocks()[0];
    if let BlockSource::Tool { call, .. } = &block.source {
        assert!(call.summary.contains("namespace"));
        assert!(!call.summary.contains("api-0"));
        assert!(!call.summary.contains("[{"));
    } else {
        panic!("Expected Tool variant");
    }
}

#[test]
fn tool_row_materializes_immediately_on_tool_start_with_args_and_running_status() {
    let mut state = AppState::default();

    reduce_tool(
        &mut state,
        started("k8s__list_pods", r#"{"namespace":"prod"}"#),
    );

    assert_eq!(state.transcript.len(), 1);
    let block = &state.transcript.blocks()[0];
    if let BlockSource::Tool { call, .. } = &block.source {
        assert!(call.summary.contains("namespace"));
    } else {
        panic!("Expected Tool variant");
    }
    assert_eq!(
        state.transcript.blocks()[0].status,
        Some(ItemStatus::InProgress)
    );
}

#[test]
fn tool_start_nu_sets_call_line_to_code_block_with_raw_command() {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec
    reduce_tool(
        &mut state,
        started_with_call_line(
            "nu",
            r#"{"command":"ls | select name type size"}"#,
            CallLine {
                summary: String::new(),
            },
        ),
    );

    // -- Check: the command renders in the preview block, so the call line
    // carries no summary.
    let block = &state.transcript.blocks()[0];
    if let BlockSource::Tool { call, .. } = &block.source {
        assert_eq!(call.summary, String::new());
    } else {
        panic!("Expected Tool variant");
    }
}

#[test]
fn tool_start_nu_multi_line_command_preserves_newlines_in_call_line() {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec
    reduce_tool(
        &mut state,
        started_with_call_line(
            "nu",
            r#"{"command":"ls | where size > 1mb\n| select name type\n| sort-by modified"}"#,
            CallLine {
                summary: String::new(),
            },
        ),
    );

    // -- Check: a multi-line command does not leak onto the call line either.
    let block = &state.transcript.blocks()[0];
    if let BlockSource::Tool { call, .. } = &block.source {
        assert_eq!(call.summary, String::new());
    } else {
        panic!("Expected Tool variant");
    }
}

#[test]
fn tool_start_nu_without_command_key_falls_back_to_summary_arrow() {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec
    reduce_tool(&mut state, started("nu", r#"{"timeout_seconds":5}"#));

    // -- Check
    let block = &state.transcript.blocks()[0];
    if let BlockSource::Tool { call, .. } = &block.source {
        assert!(
            call.summary.starts_with("→ "),
            "fallback must use the arrow summary, got: {:?}",
            call.summary
        );
    } else {
        panic!("Expected Tool variant");
    }
}

#[test]
fn tool_start_non_nu_keeps_args_summary_arrow() {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec
    reduce_tool(
        &mut state,
        started("k8s__list_pods", r#"{"namespace":"prod"}"#),
    );

    // -- Check
    let block = &state.transcript.blocks()[0];
    if let BlockSource::Tool { call, .. } = &block.source {
        assert!(call.summary.starts_with("→ "));
    } else {
        panic!("Expected Tool variant");
    }
}

#[test]
fn tool_end_transitions_same_row_to_done_or_failed_status() {
    let mut state = AppState::default();

    reduce_tool(&mut state, started("gh__get_pr", r#"{"number":1}"#));
    reduce_tool(
        &mut state,
        ToolEvent::Completed {
            name: "gh__get_pr".to_string(),
            source: "mcp".to_string(),
            arguments: r#"{"number":1}"#.to_string(),
            success: true,
            result: "ok".to_string(),
            display: None,
            error_kind: None,
            message: None,
        },
    );

    // [Tool] — no leading spacer under the unified spacer rule
    assert_eq!(state.transcript.len(), 1);
    assert_eq!(state.transcript.blocks()[0].status, Some(ItemStatus::Done));

    let mut failed = AppState::default();
    reduce_tool(&mut failed, started("gh__get_pr", r#"{"number":2}"#));
    reduce_tool(
        &mut failed,
        ToolEvent::Completed {
            name: "gh__get_pr".to_string(),
            source: "mcp".to_string(),
            arguments: r#"{"number":2}"#.to_string(),
            success: false,
            result: "err".to_string(),
            display: None,
            error_kind: Some("tool_error".to_string()),
            message: Some("boom".to_string()),
        },
    );
    assert_eq!(failed.transcript.len(), 1);
    assert_eq!(
        failed.transcript.blocks()[0].status,
        Some(ItemStatus::Failed)
    );
}

#[test]
fn tool_start_leaves_status_line_empty() {
    let mut state = AppState::default();
    reduce_tool(&mut state, started("k8s__list_pods", "{}"));
    assert!(state.status.message.status_line().is_empty());
}

#[test]
fn tool_end_leaves_status_line_empty() {
    let mut state = AppState::default();
    reduce_tool(&mut state, started("k8s__list_pods", "{}"));
    reduce_tool(
        &mut state,
        ToolEvent::Completed {
            name: "k8s__list_pods".to_string(),
            source: "mcp".to_string(),
            arguments: "{}".to_string(),
            success: true,
            result: "[]".to_string(),
            display: None,
            error_kind: None,
            message: None,
        },
    );
    assert!(state.status.message.status_line().is_empty());
}

#[test]
fn tool_start_truncates_long_args_summary_with_ellipsis() {
    let mut state = AppState::default();
    let long_args = format!("{{\"payload\":\"{}\"}}", "x".repeat(300));

    reduce_tool(&mut state, started("k8s__describe", &long_args));

    // [Tool] — no leading spacer under the unified spacer rule
    assert_eq!(state.transcript.len(), 1);
    if let BlockSource::Tool { call, .. } = &state.transcript.blocks()[0].source {
        assert!(call.summary.starts_with("→ "));
        assert!(call.summary.ends_with('…'));
        assert!(call.summary.chars().count() < 180);
    } else {
        panic!("Expected Tool variant");
    }
}

#[test]
fn handle_tool_start_pushes_tool_block_without_leading_spacer() {
    let mut state = AppState::default();
    reduce_tool(&mut state, started("read", "{}"));
    assert!(matches!(
        state.transcript.blocks()[0].source,
        BlockSource::Tool { .. }
    ));
}

#[test]
fn handle_tool_end_stores_two_content_blocks_without_separators() {
    let mut state = AppState::default();
    // Two tool calls within the same block
    for name in ["read", "write"] {
        reduce_tool(&mut state, started(name, "{}"));
        reduce_tool(
            &mut state,
            ToolEvent::Completed {
                name: name.to_string(),
                source: "builtin".to_string(),
                arguments: "{}".to_string(),
                success: true,
                result: "ok".to_string(),
                display: None,
                error_kind: None,
                message: None,
            },
        );
    }

    // transcript: [Tool, Tool] — the store holds content blocks only;
    // separators are a render-time concern (SpacerStateMachine).
    assert_eq!(state.transcript.len(), 2);
    assert!(matches!(
        state.transcript.blocks()[0].source,
        BlockSource::Tool { .. }
    ));
    assert!(matches!(
        state.transcript.blocks()[1].source,
        BlockSource::Tool { .. }
    ));
}

#[test]
fn tool_start_nu_after_plain_tool_stores_two_content_blocks() {
    let mut state = AppState::default();
    // Plain tool call (no background block)
    reduce_tool(&mut state, started("read", "{}"));
    // nu tool call (renders a background block)
    reduce_tool(&mut state, started("nu", r#"{"command":"ls"}"#));

    // transcript: [Tool(read), Tool(nu)] — no Spacer blocks in the store.
    assert_eq!(state.transcript.len(), 2);
    assert!(matches!(
        state.transcript.blocks()[0].source,
        BlockSource::Tool { .. }
    ));
    assert!(matches!(
        state.transcript.blocks()[1].source,
        BlockSource::Tool { .. }
    ));
}

#[test]
fn tool_start_plain_after_nu_stores_two_content_blocks() {
    let mut state = AppState::default();
    // nu tool call (renders a background block)
    reduce_tool(&mut state, started("nu", r#"{"command":"ls"}"#));
    // plain tool call after nu
    reduce_tool(&mut state, started("read", "{}"));

    // transcript: [Tool(nu), Tool(read)] — no Spacer blocks in the store.
    assert_eq!(state.transcript.len(), 2);
    assert!(matches!(
        state.transcript.blocks()[0].source,
        BlockSource::Tool { .. }
    ));
    assert!(matches!(
        state.transcript.blocks()[1].source,
        BlockSource::Tool { .. }
    ));
}

#[test]
fn tool_start_two_plain_tools_store_two_content_blocks() {
    let mut state = AppState::default();
    reduce_tool(&mut state, started("read", "{}"));
    reduce_tool(&mut state, started("write", "{}"));

    // transcript: [Tool(read), Tool(write)] — no Spacer blocks in the store.
    assert_eq!(state.transcript.len(), 2);
    assert!(matches!(
        state.transcript.blocks()[0].source,
        BlockSource::Tool { .. }
    ));
    assert!(matches!(
        state.transcript.blocks()[1].source,
        BlockSource::Tool { .. }
    ));
}

#[test]
fn bookkeeping_start_finish_tracks_row_status() {
    let mut state = AppState::default();
    let mut evicted = 0usize;
    state.tool.start_tool_call(
        &mut state.transcript,
        "k8s__list_pods",
        r#"{"namespace":"prod"}"#,
        CallLine::from_json_summary(r#"{"namespace":"prod"}"#),
        &mut evicted,
    );
    assert_eq!(state.transcript.len(), 1);
    state.tool.finish_tool_call(
        &mut state.transcript,
        "k8s__list_pods",
        r#"{"namespace":"prod"}"#,
        Some(true),
    );
    assert_eq!(state.transcript.blocks()[0].status, Some(ItemStatus::Done));
}

#[test]
fn bookkeeping_start_finish_unknown_renders_unknown_status() {
    let mut state = AppState::default();
    let mut evicted = 0usize;
    state.tool.start_tool_call(
        &mut state.transcript,
        "k8s__list_pods",
        r#"{"namespace":"prod"}"#,
        CallLine::from_json_summary(r#"{"namespace":"prod"}"#),
        &mut evicted,
    );
    state.tool.finish_tool_call(
        &mut state.transcript,
        "k8s__list_pods",
        r#"{"namespace":"prod"}"#,
        None,
    );
    assert_eq!(
        state.transcript.blocks()[0].status,
        Some(ItemStatus::Unknown),
        "flag-absent tool rows must render unknown, not guessed success"
    );
}

#[test]
fn concurrent_same_name_tool_calls_get_correct_statuses() {
    let mut state = AppState::default();

    // Start two tool calls with the same name but different arguments
    let mut evicted = 0usize;
    state.tool.start_tool_call(
        &mut state.transcript,
        "k8s__get_pod",
        r#"{"name":"api-0"}"#,
        CallLine::from_json_summary(r#"{"name":"api-0"}"#),
        &mut evicted,
    );
    state.tool.start_tool_call(
        &mut state.transcript,
        "k8s__get_pod",
        r#"{"name":"api-1"}"#,
        CallLine::from_json_summary(r#"{"name":"api-1"}"#),
        &mut evicted,
    );

    // Both should be InProgress. The store holds two content blocks; no
    // Separator blocks are inserted.
    assert_eq!(state.transcript.len(), 2);
    assert_eq!(
        state.transcript.blocks()[0].status,
        Some(ItemStatus::InProgress)
    );
    assert_eq!(
        state.transcript.blocks()[1].status,
        Some(ItemStatus::InProgress)
    );

    // Finish in reverse order
    state.tool.finish_tool_call(
        &mut state.transcript,
        "k8s__get_pod",
        r#"{"name":"api-1"}"#,
        Some(true),
    );
    state.tool.finish_tool_call(
        &mut state.transcript,
        "k8s__get_pod",
        r#"{"name":"api-0"}"#,
        Some(false),
    );

    // Each should get the correct status
    assert_eq!(
        state.transcript.blocks()[0].status,
        Some(ItemStatus::Failed)
    );
    assert_eq!(state.transcript.blocks()[1].status, Some(ItemStatus::Done));
}
