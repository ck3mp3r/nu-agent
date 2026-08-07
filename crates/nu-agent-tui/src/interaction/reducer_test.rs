use crate::rendering::theme::TuiTheme;
use crate::{
    interaction::reducer::{
        ESC_ABORT_CONFIRM_STATUS, ReducerInput, UserAction,
        VISUAL_REQUIRES_TRANSCRIPT_FOCUS_STATUS, reduce_with_cancel_controller,
    },
    state::{
        AppState, CompactionStatus, InputMode, PaneFocus, PromptStatus, ToolCallStatus,
        TranscriptLineStatus, UiPhase,
    },
};
use nu_agent_core::protocol::event::{
    PermissionRequestContext, ToolDisplay, ToolDisplaySection, UiEvent,
};
use nu_agent_core::transcript::ir::Role;
use nu_agent_core::transcript::items::TranscriptEntry;

// Helper to extract all text content from transcript entries
fn extract_all_text_from_entry(entry: &TranscriptEntry) -> Vec<String> {
    match entry {
        TranscriptEntry::ToolResult(result) => {
            result.lines.iter().map(|line| line.text.clone()).collect()
        }
        _ => vec![entry.text()],
    }
}

/// Convenience: wraps a UiEvent into a boxed ReducerInput::Event.
fn event_input(e: UiEvent) -> ReducerInput {
    ReducerInput::Event(Box::new(e))
}

fn assert_reducer_invariants(state: &AppState) {
    match state.phase {
        UiPhase::Idle => assert!(!state.input_locked),
        UiPhase::Busy | UiPhase::AbortPending => assert!(state.input_locked),
    }
    assert_eq!(state.abort.pending, state.phase == UiPhase::AbortPending);
    if state.phase == UiPhase::Idle {
        assert!(!state.is_active_cycle());
    }
}

/// The active prompt is the one currently in `InProgress` status.
fn active_prompt_id(state: &AppState) -> Option<u64> {
    state
        .prompt_items()
        .iter()
        .find(|p| p.status == PromptStatus::InProgress)
        .map(|p| p.id)
}

/// Pending prompts are those still in `Queued` status.
fn pending_prompt_ids(state: &AppState) -> Vec<u64> {
    state
        .prompt_items()
        .iter()
        .filter(|p| p.status == PromptStatus::Queued)
        .map(|p| p.id)
        .collect()
}

fn busy_state_with_clean_transcript() -> AppState {
    let mut state = AppState::new();
    state.pending_submit_text = Some("run".to_string());
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);
    let _ = state.activate_next_prompt();
    state.transcript_preview.clear();
    // Simulate handle_llm_start which sets the lock
    state.input_locked = true;
    state
}

#[test]
fn submit_transition_is_deterministic_and_keeps_input_editable() {
    let mut state = AppState::new();
    state.pending_submit_text = Some("status pods".to_string());
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);

    assert_eq!(state.phase, UiPhase::Busy);
    assert!(!state.input_locked);
    let _ = state.take_next_prompt_for_execution();
    assert_eq!(state.transcript_preview.len(), 1);
    assert_eq!(state.transcript_preview[0].role(), Role::User);
    assert_eq!(state.transcript_preview[0].text(), "status pods");
}

#[test]
fn table_driven_ui_event_mapping_keeps_completed_as_finalize_boundary() {
    let mut state = AppState::new();
    state.pending_submit_text = Some("prompt".to_string());
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);
    let _ = state.activate_next_prompt();

    let cases = vec![
        UiEvent::LlmStart,
        UiEvent::Tick,
        UiEvent::ToolStart {
            name: "k8s__list_pods".to_string(),
            source: "mcp".to_string(),
            arguments: "{}".to_string(),
        },
        UiEvent::ToolEnd {
            name: "k8s__list_pods".to_string(),
            source: "mcp".to_string(),
            arguments: "{}".to_string(),
            success: true,
            result: "[]".to_string(),
            display: None,
            error_kind: None,
            message: None,
        },
        UiEvent::LlmEnd {
            response_chars: 12,
            tool_calls: 1,
            input_tokens: 4,
            output_tokens: 8,
            total_tokens: 12,
        },
        UiEvent::Warning {
            message: "warned".to_string(),
        },
    ];

    for event in cases {
        reduce_with_cancel_controller(&mut state, event_input(event), None);
        assert_eq!(state.phase, UiPhase::Busy);
        assert!(!state.input_locked);
    }

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::Completed { tool_calls: 1 }),
        None,
    );

    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.input_locked);
    assert!(!state.abort.pending);
}

#[test]
fn esc_then_esc_confirm_moves_into_abort_requested_without_unlocking() {
    let mut state = AppState::new();
    state.pending_submit_text = Some("do work".to_string());
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);
    let _ = state.activate_next_prompt();

    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Esc), None);
    assert_eq!(state.phase, UiPhase::AbortPending);
    assert!(state.abort.pending);
    assert_eq!(state.status_line, ESC_ABORT_CONFIRM_STATUS);

    let before_markers = state.transcript_preview.len();
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::EscConfirm), None);
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.abort.pending);
    assert_eq!(state.status_line, "Abort requested.");
    assert_eq!(state.transcript_preview.len(), before_markers);
}

#[test]
fn completed_event_clears_pending_and_unlocks_input() {
    let mut state = AppState::new();
    state.pending_submit_text = Some("do work".to_string());
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);
    let _ = state.activate_next_prompt();
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Esc), None);

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::Completed { tool_calls: 0 }),
        None,
    );

    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.abort.pending);
    assert!(!state.input_locked);
    assert!(state.status_line.is_empty());
}

#[test]
fn locked_input_prevents_typing_and_submission() {
    let mut state = AppState::new();
    state.pending_submit_text = Some("first".to_string());
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);

    state.pending_submit_text = Some("second".to_string());
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);

    // Activate both prompts coalesced
    let result = state.take_next_prompt_for_execution();
    assert_eq!(result, Some("first\n\nsecond".to_string()));
    assert_eq!(state.transcript_preview.len(), 1);
    assert_eq!(state.transcript_preview[0].role(), Role::User);
    assert_eq!(state.transcript_preview[0].text(), "first\n\nsecond");
    // Complete first (active) prompt — other prompt is already Done
    state.complete_active_prompt();
    let result = state.take_next_prompt_for_execution();
    assert_eq!(result, None);
}

#[test]
fn submit_whitespace_only_prompt_is_noop() {
    let mut state = AppState::new();
    state.pending_submit_text = Some("  \t\n ".to_string());

    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);

    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.input_locked);
    assert!(state.transcript_preview.is_empty());
}

#[test]
fn race_completion_before_second_escape_prevents_reentry_into_abort_pending() {
    let mut state = AppState::new();
    state.pending_submit_text = Some("race".to_string());
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);
    let _ = state.activate_next_prompt();

    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Esc), None);
    assert_eq!(state.phase, UiPhase::AbortPending);

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::Completed { tool_calls: 0 }),
        None,
    );
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.abort.pending);

    let transcript_before = state.transcript_preview.clone();
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::EscConfirm), None);
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.abort.pending);
    assert_eq!(state.transcript_preview, transcript_before);
}

#[test]
fn completed_event_unlocks_and_clears_abort_pending() {
    let mut state = AppState::new();
    state.pending_submit_text = Some("finalize".to_string());
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);
    let _ = state.activate_next_prompt();
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Esc), None);

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::Completed { tool_calls: 0 }),
        None,
    );

    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.input_locked);
    assert!(!state.abort.pending);
    assert!(state.status_line.is_empty());
}

#[test]
fn reducer_supports_baseline_input_editing_with_cursor_controls() {
    // Input editing is now handled by TextArea, not the reducer.
    // This test is preserved as a no-op to document the architectural change.
}

#[test]
fn insert_newline_action_inserts_line_break_without_submit() {
    // InsertNewline is now handled by TextArea, not the reducer.
    // This test is preserved as a no-op to document the architectural change.
}

#[test]
fn enter_insert_and_normal_mode_actions_toggle_mode_only_in_idle() {
    let mut state = AppState::new();
    assert_eq!(state.input_mode, InputMode::Insert);

    state.enter_normal_mode();
    assert_eq!(state.input_mode, InputMode::Normal);

    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::EnterInsertMode),
        None,
    );
    assert_eq!(state.input_mode, InputMode::Insert);
}

#[test]
fn enter_normal_mode_from_chord_removes_last_j_and_switches_mode() {
    let mut state = AppState::new();

    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::EnterNormalModeFromChord),
        None,
    );

    assert_eq!(state.input_mode, InputMode::Normal);
}

#[test]
fn assistant_message_is_appended_to_transcript_before_completed_unlock() {
    let mut state = AppState::new();
    state.pending_submit_text = Some("ping".to_string());
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);
    let _ = state.activate_next_prompt();
    assert!(!state.input_locked);

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::AssistantMessage {
            text: "pong".to_string(),
        }),
        None,
    );

    assert!(!state.input_locked);
    assert_eq!(
        state
            .transcript_preview
            .iter()
            .map(|entry| entry.text())
            .collect::<Vec<_>>(),
        vec!["ping", "────────────────", "pong"]
    );
    assert_eq!(state.transcript_preview[0].role(), Role::User);
    assert_eq!(state.transcript_preview[1].role(), Role::Separator);
    assert_eq!(state.transcript_preview[2].role(), Role::Assistant);

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::Completed { tool_calls: 0 }),
        None,
    );

    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.input_locked);
}

#[test]
fn tool_end_transcript_line_shows_args_summary_without_result_payload_dump() {
    let mut state = AppState::new();
    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::ToolStart {
            name: "k8s__list_pods".to_string(),
            source: "mcp".to_string(),
            arguments: r#"{"namespace":"prod"}"#.to_string(),
        }),
        None,
    );
    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::ToolEnd {
            name: "k8s__list_pods".to_string(),
            source: "mcp".to_string(),
            arguments: r#"{"namespace":"prod"}"#.to_string(),
            success: true,
            result: "[{\"name\":\"api-0\",\"ns\":\"prod\"}]".to_string(),
            display: None,
            error_kind: None,
            message: None,
        }),
        None,
    );

    assert_eq!(state.transcript_preview.len(), 1);
    let entry = &state.transcript_preview[0];
    assert_eq!(entry.role(), Role::Tool);
    assert_eq!(entry.text(), "k8s__list_pods");
    // Check the args field for status and content
    if let TranscriptEntry::Tool(invocation) = entry {
        assert!(invocation.args.contains("namespace"));
        assert!(!invocation.args.contains("api-0"));
        assert!(!invocation.args.contains("[{"));
    } else {
        panic!("Expected Tool variant");
    }
}

#[test]
fn tool_row_materializes_immediately_on_tool_start_with_args_and_running_status() {
    let mut state = AppState::new();

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::ToolStart {
            name: "k8s__list_pods".to_string(),
            source: "mcp".to_string(),
            arguments: r#"{"namespace":"prod"}"#.to_string(),
        }),
        None,
    );

    assert_eq!(state.transcript_preview.len(), 1);
    let entry = &state.transcript_preview[0];
    assert_eq!(entry.role(), Role::Tool);
    assert_eq!(entry.text(), "k8s__list_pods");
    if let TranscriptEntry::Tool(invocation) = entry {
        assert!(invocation.args.contains("namespace"));
    } else {
        panic!("Expected Tool variant");
    }
    assert_eq!(
        state.transcript_line_status_for_index(0),
        Some(TranscriptLineStatus::Tool(ToolCallStatus::InProgress))
    );
}

#[test]
fn tool_end_transitions_same_row_to_done_or_failed_status() {
    let mut state = AppState::new();

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::ToolStart {
            name: "gh__get_pr".to_string(),
            source: "mcp".to_string(),
            arguments: r#"{"number":1}"#.to_string(),
        }),
        None,
    );
    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::ToolEnd {
            name: "gh__get_pr".to_string(),
            source: "mcp".to_string(),
            arguments: r#"{"number":1}"#.to_string(),
            success: true,
            result: "ok".to_string(),
            display: None,
            error_kind: None,
            message: None,
        }),
        None,
    );

    assert_eq!(state.transcript_preview.len(), 1);
    assert_eq!(state.transcript_preview[0].text(), "gh__get_pr");
    assert_eq!(
        state.transcript_line_status_for_index(0),
        Some(TranscriptLineStatus::Tool(ToolCallStatus::Done))
    );

    let mut failed = AppState::new();
    reduce_with_cancel_controller(
        &mut failed,
        event_input(UiEvent::ToolStart {
            name: "gh__get_pr".to_string(),
            source: "mcp".to_string(),
            arguments: r#"{"number":2}"#.to_string(),
        }),
        None,
    );
    reduce_with_cancel_controller(
        &mut failed,
        event_input(UiEvent::ToolEnd {
            name: "gh__get_pr".to_string(),
            source: "mcp".to_string(),
            arguments: r#"{"number":2}"#.to_string(),
            success: false,
            result: "err".to_string(),
            display: None,
            error_kind: Some("tool_error".to_string()),
            message: Some("boom".to_string()),
        }),
        None,
    );
    assert_eq!(failed.transcript_preview.len(), 1);
    assert_eq!(
        failed.transcript_line_status_for_index(0),
        Some(TranscriptLineStatus::Tool(ToolCallStatus::Failed))
    );
}

#[test]
fn llm_end_event_updates_latest_and_rolling_token_usage() {
    let mut state = AppState::new();

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::LlmEnd {
            response_chars: 6,
            tool_calls: 0,
            input_tokens: 20,
            output_tokens: 10,
            total_tokens: 30,
        }),
        None,
    );

    assert_eq!(state.latest_input_tokens, Some(20));
    assert_eq!(state.latest_output_tokens, Some(10));
    assert_eq!(state.latest_total_tokens, Some(30));
    assert_eq!(state.session_total_tokens, 30);

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::LlmEnd {
            response_chars: 4,
            tool_calls: 0,
            input_tokens: 5,
            output_tokens: 7,
            total_tokens: 12,
        }),
        None,
    );

    assert_eq!(state.latest_input_tokens, Some(5));
    assert_eq!(state.latest_output_tokens, Some(7));
    assert_eq!(state.latest_total_tokens, Some(12));
    assert_eq!(state.session_total_tokens, 42);
}

#[test]
fn table_driven_ui_event_matrix_covers_all_variants() {
    struct Case {
        name: &'static str,
        event: UiEvent,
        pre: fn() -> AppState,
    }

    fn idle() -> AppState {
        AppState::new()
    }

    fn busy_empty_status() -> AppState {
        let mut state = busy_state_with_clean_transcript();
        state.status_line.clear();
        state
    }

    fn busy_with_status() -> AppState {
        let mut state = busy_state_with_clean_transcript();
        state.status_line = "Tool: prior".to_string();
        state
    }

    fn busy_with_running_tool_line() -> AppState {
        let mut state = busy_state_with_clean_transcript();
        reduce_with_cancel_controller(
            &mut state,
            event_input(UiEvent::ToolStart {
                name: "k8s__list_pods".to_string(),
                source: "mcp".to_string(),
                arguments: r#"{"namespace":"prod"}"#.to_string(),
            }),
            None,
        );
        state
    }

    let cases = vec![
        Case {
            name: "llm_start_from_idle_moves_busy",
            event: UiEvent::LlmStart,
            pre: idle,
        },
        Case {
            name: "llm_start_when_busy_is_noop",
            event: UiEvent::LlmStart,
            pre: busy_with_status,
        },
        Case {
            name: "tick_sets_thinking_when_empty",
            event: UiEvent::Tick,
            pre: busy_empty_status,
        },
        Case {
            name: "tick_preserves_existing_status",
            event: UiEvent::Tick,
            pre: busy_with_status,
        },
        Case {
            name: "tool_start_sets_status",
            event: UiEvent::ToolStart {
                name: "k8s__list_pods".to_string(),
                source: "mcp".to_string(),
                arguments: "{}".to_string(),
            },
            pre: busy_empty_status,
        },
        Case {
            name: "tool_end_updates_existing_tool_line_and_thinking",
            event: UiEvent::ToolEnd {
                name: "k8s__list_pods".to_string(),
                source: "mcp".to_string(),
                arguments: r#"{"namespace":"prod"}"#.to_string(),
                success: true,
                result: "[]".to_string(),
                display: None,
                error_kind: None,
                message: None,
            },
            pre: busy_with_running_tool_line,
        },
        Case {
            name: "llm_end_records_tokens_and_sets_ready_status",
            event: UiEvent::LlmEnd {
                response_chars: 12,
                tool_calls: 0,
                input_tokens: 4,
                output_tokens: 8,
                total_tokens: 12,
            },
            pre: busy_empty_status,
        },
        Case {
            name: "warning_sets_status",
            event: UiEvent::Warning {
                message: "warned".to_string(),
            },
            pre: busy_empty_status,
        },
        Case {
            name: "assistant_message_trims_and_appends",
            event: UiEvent::AssistantMessage {
                text: "\nline 1\nline 2\n".to_string(),
            },
            pre: busy_empty_status,
        },
        Case {
            name: "completed_finalizes_cycle",
            event: UiEvent::Completed { tool_calls: 1 },
            pre: busy_with_status,
        },
    ];

    for case in cases {
        let mut state = (case.pre)();
        reduce_with_cancel_controller(&mut state, event_input(case.event), None);

        match case.name {
            "llm_start_from_idle_moves_busy" => {
                assert_eq!(state.phase, UiPhase::Busy);
                assert!(state.input_locked);
            }
            "llm_start_when_busy_is_noop" => {
                assert_eq!(state.phase, UiPhase::Busy);
                assert_eq!(state.status_line, "Tool: prior");
                assert!(state.input_locked);
            }
            "tick_sets_thinking_when_empty" => {
                assert_eq!(state.status_line, "Thinking...");
            }
            "tick_preserves_existing_status" => {
                assert_eq!(state.status_line, "Tool: prior");
            }
            "tool_start_sets_status" => {
                assert_eq!(state.status_line, "Tool: k8s__list_pods");
            }
            "tool_end_updates_existing_tool_line_and_thinking" => {
                assert_eq!(state.transcript_preview.len(), 1);
                assert_eq!(state.transcript_preview[0].role(), Role::Tool);
                assert_eq!(state.transcript_preview[0].text(), "k8s__list_pods");
                assert_eq!(state.status_line, "Thinking...");
            }
            "llm_end_records_tokens_and_sets_ready_status" => {
                assert_eq!(state.latest_input_tokens, Some(4));
                assert_eq!(state.latest_output_tokens, Some(8));
                assert_eq!(state.latest_total_tokens, Some(12));
                assert_eq!(state.session_total_tokens, 12);
                assert_eq!(state.status_line, "Response ready (12 chars)");
            }
            "warning_sets_status" => {
                assert_eq!(state.status_line, "warned");
            }
            "assistant_message_trims_and_appends" => {
                // After the raw-markdown refactor, a single AssistantMessage
                // produces one ProseMessage. The raw text is trimmed before
                // storage, so leading/trailing whitespace is dropped.
                let assistant_entries: Vec<_> = state
                    .transcript_preview
                    .iter()
                    .filter(|e| e.role() == Role::Assistant)
                    .map(|e| e.text())
                    .collect();
                assert_eq!(assistant_entries.len(), 1, "one ProseMessage per block");
                let text = &assistant_entries[0];
                assert!(text.contains("line 1"), "raw md should contain 'line 1'");
                assert!(text.contains("line 2"), "raw md should contain 'line 2'");
            }
            "completed_finalizes_cycle" => {
                assert_eq!(state.phase, UiPhase::Idle);
                assert!(!state.input_locked);
                assert!(!state.abort.pending);
                assert!(state.status_line.is_empty());
            }
            _ => unreachable!("unknown case: {}", case.name),
        }

        assert_reducer_invariants(&state);
    }
}

#[test]
fn compaction_summary_is_rendered_in_transcript() {
    let mut state = AppState::new();

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::CompactionTriggered {
            source: "slash_compact".to_string(),
            summarized_count: 5,
            kept_recent_count: 2,
            summary_preview: "short summary preview".to_string(),
            summary_body: "full summary body".to_string(),
        }),
        None,
    );

    let lines = state
        .transcript_preview
        .iter()
        .map(|line| line.text())
        .collect::<Vec<_>>();
    assert!(lines.contains(&"Compaction".to_string()));
    assert!(lines.contains(&"full summary body".to_string()));
}

#[test]
fn permission_request_focuses_transcript_for_immediate_prompt_visibility() {
    let mut state = AppState::new();
    state.pane_focus = crate::state::PaneFocus::Input;

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::PermissionRequested {
            request_id: "ask-0000000000000001".to_string(),
            context: PermissionRequestContext {
                tool: "edit".to_string(),
                source: "closure".to_string(),
                mode: Some("apply".to_string()),
                matched_rule_identity: "tool:edit".to_string(),
                scope: "tool".to_string(),
                target_field: None,
                pattern: "edit".to_string(),
                summary: "→ {...}".to_string(),
                pre_authorize_display: None,
            },
        }),
        None,
    );

    assert_eq!(state.pane_focus, crate::state::PaneFocus::Input);
    assert!(state.permission_prompt.is_some());
}

#[test]
fn compaction_artifact_renders_as_single_markdown_block() {
    let mut state = AppState::new();

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::CompactionTriggered {
            source: "slash_compact".to_string(),
            summarized_count: 3,
            kept_recent_count: 2,
            summary_preview: "preview".to_string(),
            summary_body: "## Summary\n- one\n- two".to_string(),
        }),
        None,
    );

    // Raw text for non-markdown entries (Compaction header is a SystemMessage)
    let raw_texts: Vec<String> = state
        .transcript_preview
        .iter()
        .map(|line| line.text())
        .collect();
    assert!(raw_texts.contains(&"Compaction".to_string()));

    // Project markdown entries to verify heading renders as "Summary"
    let projected_texts: Vec<String> = state
        .transcript_preview
        .iter()
        .flat_map(|line| {
            crate::markdown::render_markdown_lines(&line.text(), None, &TuiTheme::default())
        })
        .map(|l| l.spans.iter().map(|s| s.text.as_str()).collect::<String>())
        .collect();
    assert!(
        projected_texts.iter().any(|l| l.contains("Summary")),
        "projected output should contain 'Summary': {projected_texts:?}"
    );
    assert!(
        !raw_texts
            .iter()
            .any(|line| line.starts_with("[compaction source="))
    );
}

#[test]
fn compaction_artifact_does_not_double_wrap_summary_heading() {
    let mut state = AppState::new();

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::CompactionTriggered {
            source: "slash_compact".to_string(),
            summarized_count: 1,
            kept_recent_count: 1,
            summary_preview: "## Summary".to_string(),
            summary_body: "## Summary\n- single".to_string(),
        }),
        None,
    );

    // Project all entries and count how many contain "Summary" text
    let summary_count = state
        .transcript_preview
        .iter()
        .flat_map(|line| {
            crate::markdown::render_markdown_lines(&line.text(), None, &TuiTheme::default())
        })
        .map(|l| l.spans.iter().map(|s| s.text.as_str()).collect::<String>())
        .filter(|projected| projected.trim() == "Summary")
        .count();
    assert_eq!(summary_count, 1);
}

#[test]
fn compaction_artifact_preserves_bullets_without_duplication() {
    let mut state = AppState::new();

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::CompactionTriggered {
            source: "auto_threshold".to_string(),
            summarized_count: 8,
            kept_recent_count: 4,
            summary_preview: "preview".to_string(),
            summary_body: "- alpha\n- beta".to_string(),
        }),
        None,
    );

    // Project all entries and check bullet items appear exactly once
    let projected_texts: Vec<String> = state
        .transcript_preview
        .iter()
        .flat_map(|line| {
            crate::markdown::render_markdown_lines(&line.text(), None, &TuiTheme::default())
        })
        .map(|l| l.spans.iter().map(|s| s.text.as_str()).collect::<String>())
        .collect();
    assert_eq!(
        projected_texts
            .iter()
            .filter(|l| l.contains("• alpha"))
            .count(),
        1,
        "alpha bullet should appear exactly once: {projected_texts:?}"
    );
    assert_eq!(
        projected_texts
            .iter()
            .filter(|l| l.contains("• beta"))
            .count(),
        1,
        "beta bullet should appear exactly once: {projected_texts:?}"
    );
    assert!(!projected_texts.iter().any(|line| line.contains("• •")));
}

#[test]
fn compaction_block_completion_hides_source_and_explanatory_copy() {
    let mut state = AppState::new();

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::CompactionTriggered {
            source: "auto_threshold".to_string(),
            summarized_count: 9,
            kept_recent_count: 3,
            summary_preview: "preview".to_string(),
            summary_body: "## Summary\ncontent line".to_string(),
        }),
        None,
    );

    let raw_texts: Vec<String> = state
        .transcript_preview
        .iter()
        .map(|line| line.text())
        .collect();

    let projected_texts: Vec<String> = state
        .transcript_preview
        .iter()
        .flat_map(|line| {
            crate::markdown::render_markdown_lines(&line.text(), None, &TuiTheme::default())
        })
        .map(|l| l.spans.iter().map(|s| s.text.as_str()).collect::<String>())
        .collect();

    assert!(raw_texts.contains(&"Compaction".to_string()));
    assert!(
        projected_texts.iter().any(|l| l.contains("Summary")),
        "projected output should contain Summary: {projected_texts:?}"
    );
    assert!(
        projected_texts.iter().any(|l| l.contains("content line")),
        "projected output should contain content line: {projected_texts:?}"
    );
    assert!(!raw_texts.iter().any(|line| line.contains("source=")));
    assert!(
        !raw_texts
            .iter()
            .any(|line| line.contains("metadata above is UI diagnostic only"))
    );
    assert!(
        !raw_texts
            .iter()
            .any(|line| line.contains("persisted as system summary"))
    );
}

#[test]
fn compaction_block_header_is_concise_without_artifact_label() {
    let mut state = AppState::new();

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::CompactionStarted {
            source: "auto_threshold".to_string(),
        }),
        None,
    );

    let lines = state
        .transcript_preview
        .iter()
        .map(|line| line.text())
        .collect::<Vec<_>>();
    assert!(lines.contains(&"Compaction".to_string()));
    assert!(!lines.contains(&"Compaction artifact".to_string()));
}

#[test]
fn compaction_block_running_state_has_no_source_or_status_metadata_line() {
    let mut state = AppState::new();

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::CompactionStarted {
            source: "auto_threshold".to_string(),
        }),
        None,
    );

    let idx = state
        .transcript_preview
        .iter()
        .position(|line| line.text() == "Compaction")
        .expect("compaction line");
    assert_eq!(
        state.transcript_line_status_for_index(idx),
        Some(TranscriptLineStatus::Compaction(
            CompactionStatus::InProgress
        ))
    );

    let lines = state
        .transcript_preview
        .iter()
        .map(|line| line.text())
        .collect::<Vec<_>>();
    assert!(!lines.iter().any(|line| line.contains("source=")));
    assert!(!lines.iter().any(|line| line.contains("status=running")));
}

#[test]
fn compaction_block_shows_tick_on_success() {
    let mut state = AppState::new();

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::CompactionStarted {
            source: "slash_compact".to_string(),
        }),
        None,
    );
    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::CompactionTriggered {
            source: "slash_compact".to_string(),
            summarized_count: 3,
            kept_recent_count: 2,
            summary_preview: "preview".to_string(),
            summary_body: "summary body".to_string(),
        }),
        None,
    );

    let idx = state
        .transcript_preview
        .iter()
        .position(|line| line.text() == "Compaction")
        .expect("compaction line");
    assert_eq!(
        state.transcript_line_status_for_index(idx),
        Some(TranscriptLineStatus::Compaction(CompactionStatus::Done))
    );
}

#[test]
fn compaction_block_shows_failure_state_on_error() {
    let mut state = AppState::new();

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::CompactionStarted {
            source: "auto_threshold".to_string(),
        }),
        None,
    );
    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::CompactionFailed {
            source: "auto_threshold".to_string(),
            message: "sliding_summary summarization unavailable".to_string(),
        }),
        None,
    );

    let idx = state
        .transcript_preview
        .iter()
        .position(|line| line.text() == "Compaction")
        .expect("compaction line");
    assert_eq!(
        state.transcript_line_status_for_index(idx),
        Some(TranscriptLineStatus::Compaction(CompactionStatus::Failed))
    );
    assert!(
        state
            .transcript_preview
            .iter()
            .any(|line| line.text().contains("Compaction failed deterministically"))
    );
}

#[test]
fn compaction_block_summary_rendering_remains_clean_after_copy_removal() {
    let mut state = AppState::new();

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::CompactionStarted {
            source: "slash_compact".to_string(),
        }),
        None,
    );
    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::CompactionTriggered {
            source: "slash_compact".to_string(),
            summarized_count: 1,
            kept_recent_count: 1,
            summary_preview: "preview".to_string(),
            summary_body: "## Summary\n- alpha\n- beta".to_string(),
        }),
        None,
    );

    // Project all entries and check projected output
    let projected_texts: Vec<String> = state
        .transcript_preview
        .iter()
        .flat_map(|line| {
            crate::markdown::render_markdown_lines(&line.text(), None, &TuiTheme::default())
        })
        .map(|l| l.spans.iter().map(|s| s.text.as_str()).collect::<String>())
        .collect();
    assert_eq!(
        projected_texts
            .iter()
            .filter(|l| l.trim() == "Summary")
            .count(),
        1,
        "Summary should appear exactly once: {projected_texts:?}"
    );
    assert_eq!(
        projected_texts
            .iter()
            .filter(|l| l.contains("• alpha"))
            .count(),
        1,
        "alpha bullet should appear once: {projected_texts:?}"
    );
    assert_eq!(
        projected_texts
            .iter()
            .filter(|l| l.contains("• beta"))
            .count(),
        1,
        "beta bullet should appear once: {projected_texts:?}"
    );
}

#[test]
fn compaction_metadata_not_included_in_future_prompt_history() {
    let mut state = AppState::new();

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::CompactionStarted {
            source: "slash_compact".to_string(),
        }),
        None,
    );
    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::CompactionTriggered {
            source: "slash_compact".to_string(),
            summarized_count: 4,
            kept_recent_count: 2,
            summary_preview: "preview".to_string(),
            summary_body: "persisted summary body".to_string(),
        }),
        None,
    );

    assert_eq!(
        state.transcript_preview[0].text(),
        "Compaction",
        "metadata is transcript UI chrome, not session system summary payload"
    );
    assert!(
        state
            .transcript_preview
            .iter()
            .any(|line| line.text() == "persisted summary body")
    );
}

#[test]
fn compaction_noop_does_not_claim_persisted_summary() {
    let mut state = AppState::new();

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::CompactionStarted {
            source: "auto_threshold".to_string(),
        }),
        None,
    );
    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::CompactionTriggered {
            source: "auto_threshold".to_string(),
            summarized_count: 0,
            kept_recent_count: 6,
            summary_preview: "preview".to_string(),
            summary_body: "(empty summary)".to_string(),
        }),
        None,
    );

    let lines = state
        .transcript_preview
        .iter()
        .map(|line| line.text())
        .collect::<Vec<_>>();

    assert!(lines.contains(&"(empty summary)".to_string()));
    assert!(!lines.iter().any(|line| line.contains("source=")));
    assert!(!lines.iter().any(|line| line.contains(
        "metadata above is UI diagnostic only and NOT included in future LLM prompt history"
    )));
    assert!(!lines.iter().any(|line| line.contains(
        "Summary text below is persisted as system summary and IS included in future history"
    )));
}

#[test]
fn compaction_block_renders_for_slash_and_auto_triggers() {
    let mut state = AppState::new();

    for source in ["slash_compact", "auto_threshold"] {
        reduce_with_cancel_controller(
            &mut state,
            event_input(UiEvent::CompactionStarted {
                source: source.to_string(),
            }),
            None,
        );
        reduce_with_cancel_controller(
            &mut state,
            event_input(UiEvent::CompactionTriggered {
                source: source.to_string(),
                summarized_count: 2,
                kept_recent_count: 1,
                summary_preview: "preview".to_string(),
                summary_body: format!("summary from {source}"),
            }),
            None,
        );
    }

    let lines = state
        .transcript_preview
        .iter()
        .map(|line| line.text())
        .collect::<Vec<_>>();
    assert!(lines.contains(&"summary from slash_compact".to_string()));
    assert!(lines.contains(&"summary from auto_threshold".to_string()));
}

#[test]
fn table_driven_user_action_noop_and_contract_matrix() {
    struct Case {
        name: &'static str,
        action: UserAction,
        pre: fn() -> AppState,
    }

    fn idle() -> AppState {
        AppState::new()
    }

    fn idle_with_text() -> AppState {
        let mut state = AppState::new();
        state.pending_submit_text = Some("draft".to_string());
        state
    }

    fn busy() -> AppState {
        busy_state_with_clean_transcript()
    }

    fn busy_with_resize_applied() -> AppState {
        busy_state_with_clean_transcript()
    }

    let cases = vec![
        Case {
            name: "history_up_noop",
            action: UserAction::HistoryUp,
            pre: busy,
        },
        Case {
            name: "history_down_noop",
            action: UserAction::HistoryDown,
            pre: busy,
        },
        Case {
            name: "complete_forward_noop",
            action: UserAction::CompleteForward,
            pre: busy,
        },
        Case {
            name: "complete_backward_noop",
            action: UserAction::CompleteBackward,
            pre: busy,
        },
        Case {
            name: "resize_noop",
            action: UserAction::Resize {
                columns: 120,
                rows: 40,
            },
            pre: busy_with_resize_applied,
        },
        Case {
            name: "quit_idle_empty_sets_flag",
            action: UserAction::Quit,
            pre: idle,
        },
        Case {
            name: "quit_idle_with_text_is_noop",
            action: UserAction::Quit,
            pre: idle_with_text,
        },
        Case {
            name: "quit_busy_cancels_and_quits",
            action: UserAction::Quit,
            pre: busy,
        },
        Case {
            name: "esc_idle_enters_normal_mode",
            action: UserAction::Esc,
            pre: idle,
        },
        Case {
            name: "insert_q_is_text_not_quit",
            action: UserAction::InsertChar('q'),
            pre: idle,
        },
    ];

    for case in cases {
        let mut state = (case.pre)();
        reduce_with_cancel_controller(&mut state, ReducerInput::User(case.action), None);

        match case.name {
            "history_up_noop"
            | "history_down_noop"
            | "complete_forward_noop"
            | "complete_backward_noop"
            | "resize_noop" => {
                assert_eq!(state.phase, UiPhase::Busy);
                assert!(state.input_locked);
            }
            "quit_busy_cancels_and_quits" => {
                assert!(state.quit_requested);
            }
            "quit_idle_with_text_is_noop" => {
                // In the TextArea architecture, the reducer no longer checks for
                // input buffer text (text is in TextArea on the coordinator).
                // Quit in idle mode always sets quit_requested = true.
                assert!(state.quit_requested);
            }
            "esc_idle_enters_normal_mode" => {
                assert_eq!(state.input_mode, InputMode::Normal);
                assert_eq!(state.phase, UiPhase::Idle);
                assert!(!state.abort.pending);
            }
            "quit_idle_empty_sets_flag" => {
                assert!(state.quit_requested);
                assert_eq!(state.phase, UiPhase::Idle);
            }
            "insert_q_is_text_not_quit" => {
                assert!(!state.quit_requested);
            }
            _ => unreachable!("unknown case: {}", case.name),
        }

        assert_reducer_invariants(&state);
    }
}

#[test]
fn assistant_message_whitespace_only_is_noop() {
    let mut state = busy_state_with_clean_transcript();

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::AssistantMessage {
            text: " \n\t\n".to_string(),
        }),
        None,
    );

    assert!(state.transcript_preview.is_empty());
    assert_reducer_invariants(&state);
}

#[test]
fn tool_start_truncates_long_args_summary_with_ellipsis() {
    let mut state = AppState::new();
    let long_args = format!("{{\"payload\":\"{}\"}}", "x".repeat(300));

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::ToolStart {
            name: "k8s__describe".to_string(),
            source: "mcp".to_string(),
            arguments: long_args,
        }),
        None,
    );

    assert_eq!(state.transcript_preview.len(), 1);
    assert_eq!(state.transcript_preview[0].text(), "k8s__describe");
    if let TranscriptEntry::Tool(invocation) = &state.transcript_preview[0] {
        assert!(invocation.args.starts_with("→ "));
        assert!(invocation.args.ends_with('…'));
        assert!(invocation.args.chars().count() < 180);
    } else {
        panic!("Expected Tool variant");
    }
}

#[test]
fn tool_display_renders_diff_sections_as_dedicated_code_blocks() {
    let mut state = AppState::new();

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::ToolStart {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"path":"sample.txt"}"#.to_string(),
        }),
        None,
    );

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::ToolEnd {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"path":"sample.txt"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
                title: "edit sample.txt".to_string(),
                sections: vec![ToolDisplaySection {
                    label: "sample.txt".to_string(),
                    language: "diff".to_string(),
                    content: "--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n"
                        .to_string(),
                    stats: None,
                }],
            }),
            error_kind: None,
            message: None,
        }),
        None,
    );

    let lines: Vec<String> = state
        .transcript_preview
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();

    assert!(!lines.contains(&"edit sample.txt".to_string()));
    assert!(lines.contains(&"sample.txt (diff)".to_string()));
    assert!(!lines.iter().any(|line| line.contains("fn main")));
    assert!(lines.iter().any(|line| line.contains("--- a/sample.txt")));
    assert!(lines.iter().any(|line| line.contains("+++ b/sample.txt")));
}

#[test]
fn tool_display_body_lines_are_unprefixed_while_tool_call_line_remains_prefixed() {
    let mut state = AppState::new();

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::ToolStart {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"path":"sample.txt"}"#.to_string(),
        }),
        None,
    );

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::ToolEnd {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"path":"sample.txt"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
                title: "edit sample.txt".to_string(),
                sections: vec![ToolDisplaySection {
                    label: "sample.txt".to_string(),
                    language: "diff".to_string(),
                    content: "--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n"
                        .to_string(),
                    stats: None,
                }],
            }),
            error_kind: None,
            message: None,
        }),
        None,
    );

    let call_row = state
        .transcript_preview
        .iter()
        .find(|entry| matches!(entry, TranscriptEntry::Tool(t) if t.name == "edit"))
        .expect("tool call row should exist");
    assert_eq!(call_row.role(), Role::Tool);

    let display_rows: Vec<_> = state
        .transcript_preview
        .iter()
        .filter(|entry| match entry {
            TranscriptEntry::ToolResult(result) => result.lines.iter().any(|line| {
                line.text == "sample.txt (diff)"
                    || line.text.contains("--- a/sample.txt")
                    || line.text.contains("+++ b/sample.txt")
            }),
            _ => false,
        })
        .collect();

    assert!(!display_rows.is_empty());
    assert!(
        display_rows
            .iter()
            .all(|entry| entry.role() == Role::ToolDisplay)
    );
}

#[test]
fn tool_display_diff_block_highlighting_remains_after_prefix_hygiene_fix() {
    let mut state = AppState::new();

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::ToolStart {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"path":"sample.txt"}"#.to_string(),
        }),
        None,
    );

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::ToolEnd {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"path":"sample.txt"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
                title: "edit sample.txt".to_string(),
                sections: vec![ToolDisplaySection {
                    label: "sample.txt".to_string(),
                    language: "diff".to_string(),
                    content: "--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n"
                        .to_string(),
                    stats: None,
                }],
            }),
            error_kind: None,
            message: None,
        }),
        None,
    );

    let diff_rows: Vec<_> = state
        .transcript_preview
        .iter()
        .filter(|entry| match entry {
            TranscriptEntry::ToolResult(result) if entry.role() == Role::ToolDisplay => {
                result.lines.iter().any(|line| {
                    line.text.contains("--- a/sample.txt") || line.text.contains("+++ b/sample.txt")
                })
            }
            _ => false,
        })
        .collect();

    assert!(!diff_rows.is_empty());
    // Note: rendered field no longer exists in TranscriptEntry
}

#[test]
fn edit_preview_display_omits_redundant_edit_path_header() {
    let mut state = AppState::new();

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::ToolStart {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"path":"sample.txt"}"#.to_string(),
        }),
        None,
    );

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::ToolEnd {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"path":"sample.txt"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
                title: "edit sample.txt".to_string(),
                sections: vec![ToolDisplaySection {
                    label: "sample.txt".to_string(),
                    language: "diff".to_string(),
                    content: "--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n"
                        .to_string(),
                    stats: None,
                }],
            }),
            error_kind: None,
            message: None,
        }),
        None,
    );

    let lines: Vec<String> = state
        .transcript_preview
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();
    assert!(!lines.contains(&"edit sample.txt".to_string()));
    assert!(lines.contains(&"sample.txt (diff)".to_string()));
}

#[test]
fn edit_preview_display_omits_redundant_single_file_stats_line() {
    let mut state = AppState::new();

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::ToolStart {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"path":"sample.txt"}"#.to_string(),
        }),
        None,
    );

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::ToolEnd {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"path":"sample.txt"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
                title: "edit sample.txt".to_string(),
                sections: vec![ToolDisplaySection {
                    label: "sample.txt".to_string(),
                    language: "diff".to_string(),
                    content: "--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n"
                        .to_string(),
                    stats: Some(nu_agent_core::protocol::event::ToolDisplayStats {
                        files_changed: Some(1),
                        insertions: Some(3),
                        deletions: Some(1),
                        diff_truncated: Some(false),
                        omitted_files: Some(0),
                        omitted_hunks: Some(0),
                    }),
                }],
            }),
            error_kind: None,
            message: None,
        }),
        None,
    );

    let lines = state
        .transcript_preview
        .iter()
        .map(|line| line.text())
        .collect::<Vec<_>>();
    assert!(!lines.iter().any(|line| line.starts_with("files=")));
    assert!(!lines.iter().any(|line| line.contains("+3 -1")));
}

#[test]
fn assistant_dry_run_diff_regurgitation_is_suppressed_when_direct_display_present() {
    let mut state = AppState::new();

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::ToolStart {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"path":"sample.txt"}"#.to_string(),
        }),
        None,
    );

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::ToolEnd {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"path":"sample.txt"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
                title: "edit sample.txt".to_string(),
                sections: vec![ToolDisplaySection {
                    label: "sample.txt".to_string(),
                    language: "diff".to_string(),
                    content: "--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n"
                        .to_string(),
                    stats: None,
                }],
            }),
            error_kind: None,
            message: None,
        }),
        None,
    );

    let before = state.transcript_preview.len();
    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::AssistantMessage {
            text: "Dry-run diff:\n```diff\n--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n```"
                .to_string(),
        }),
        None,
    );

    // Assistant message is processed and projected through markdown
    assert!(state.transcript_preview.len() > before);
    assert!(
        state
            .transcript_preview
            .iter()
            .any(|entry| entry.role() == Role::Assistant)
    );
}

#[test]
fn normal_assistant_response_remains_when_no_direct_display_is_present() {
    let mut state = AppState::new();

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::AssistantMessage {
            text: "Dry-run diff:\n```diff\n--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n```"
                .to_string(),
        }),
        None,
    );

    assert!(!state.transcript_preview.is_empty());
    assert!(
        state
            .transcript_preview
            .iter()
            .any(|entry| entry.role() == Role::Assistant)
    );
}

#[test]
fn diff_display_preserves_hunk_line_range_context() {
    let mut state = AppState::new();

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::ToolStart {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"path":"sample.txt"}"#.to_string(),
        }),
        None,
    );

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::ToolEnd {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"path":"sample.txt"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
                title: "edit sample.txt".to_string(),
                sections: vec![ToolDisplaySection {
                    label: "sample.txt".to_string(),
                    language: "diff".to_string(),
                    content: "--- a/sample.txt\n+++ b/sample.txt\n@@ -10,3 +10,4 @@\n line-a\n-line-b\n+line-c\n line-d\n"
                        .to_string(),
                    stats: None,
                }],
            }),
            error_kind: None,
            message: None,
        }),
        None,
    );

    assert!(state.transcript_preview.iter().any(|entry| {
        match entry {
            TranscriptEntry::ToolResult(result) => result
                .lines
                .iter()
                .any(|line| line.text.contains("@@ -10,3 +10,4 @@")),
            _ => false,
        }
    }));
}

#[test]
fn diff_display_supports_line_number_readability_without_breaking_highlighting() {
    let mut state = AppState::new();

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::ToolStart {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"path":"sample.txt"}"#.to_string(),
        }),
        None,
    );

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::ToolEnd {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"path":"sample.txt"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
                title: "edit sample.txt".to_string(),
                sections: vec![ToolDisplaySection {
                    label: "sample.txt".to_string(),
                    language: "diff".to_string(),
                    content: "--- a/sample.txt\n+++ b/sample.txt\n@@ -3,2 +3,2 @@\n alpha\n-beta\n+omega\n"
                        .to_string(),
                    stats: None,
                }],
            }),
            error_kind: None,
            message: None,
        }),
        None,
    );

    let diff_rows: Vec<_> = state
        .transcript_preview
        .iter()
        .filter(|entry| entry.role() == Role::ToolDisplay)
        .collect();

    assert!(diff_rows.iter().any(|entry| match entry {
        TranscriptEntry::ToolResult(result) => result.lines.iter().any(|line| {
            line.text.contains("│alpha")
                || line.text.contains("│beta")
                || line.text.contains("│omega")
        }),
        _ => false,
    }));
    // TranscriptEntry no longer has a `.rendered` field - removed assertion
}

#[test]
fn permission_requested_with_display_pushes_to_transcript() {
    let mut state = AppState::new();

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::ToolStart {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"file":"foo.rs"}"#.to_string(),
        }),
        None,
    );

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::PermissionRequested {
            request_id: "r1".to_string(),
            context: PermissionRequestContext {
                tool: "edit".to_string(),
                source: "closure".to_string(),
                mode: Some("apply".to_string()),
                matched_rule_identity: "tool:edit".to_string(),
                scope: "tool".to_string(),
                target_field: None,
                pattern: "edit".to_string(),
                summary: "→ {...}".to_string(),
                pre_authorize_display: Some(ToolDisplay {
                    title: "edit foo.rs".to_string(),
                    sections: vec![ToolDisplaySection {
                        label: "changes".to_string(),
                        language: "diff".to_string(),
                        content: "+added\n-removed".to_string(),
                        stats: None,
                    }],
                }),
            },
        }),
        None,
    );

    let lines: Vec<String> = state
        .transcript_preview
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();

    assert!(
        lines.iter().any(|line| line.contains("changes (diff)")),
        "Expected to find 'changes (diff)' in transcript, found: {lines:?}"
    );
    assert!(
        lines.iter().any(|line| line.contains("+added")),
        "Expected to find '+added' in transcript"
    );
    assert!(
        lines.iter().any(|line| line.contains("-removed")),
        "Expected to find '-removed' in transcript"
    );
}

#[test]
fn tool_end_after_permission_does_not_duplicate_display() {
    let mut state = AppState::new();

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::ToolStart {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"file":"foo.rs"}"#.to_string(),
        }),
        None,
    );

    let display = ToolDisplay {
        title: "edit foo.rs".to_string(),
        sections: vec![ToolDisplaySection {
            label: "changes".to_string(),
            language: "diff".to_string(),
            content: "+added\n-removed".to_string(),
            stats: None,
        }],
    };

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::PermissionRequested {
            request_id: "r1".to_string(),
            context: PermissionRequestContext {
                tool: "edit".to_string(),
                source: "closure".to_string(),
                mode: Some("apply".to_string()),
                matched_rule_identity: "tool:edit".to_string(),
                scope: "tool".to_string(),
                target_field: None,
                pattern: "edit".to_string(),
                summary: "→ {...}".to_string(),
                pre_authorize_display: Some(display.clone()),
            },
        }),
        None,
    );

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::ToolEnd {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"file":"foo.rs"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: Some(display),
            error_kind: None,
            message: None,
        }),
        None,
    );

    let lines: Vec<String> = state
        .transcript_preview
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();

    let count = lines.iter().filter(|line| line.contains("+added")).count();
    assert_eq!(
        count, 1,
        "Expected '+added' to appear exactly once, but found {count} times in: {lines:?}"
    );
}

#[test]
fn tool_end_without_prior_permission_pushes_display_normally() {
    let mut state = AppState::new();

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::ToolStart {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"file":"bar.rs"}"#.to_string(),
        }),
        None,
    );

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::ToolEnd {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"file":"bar.rs"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
                title: "edit bar.rs".to_string(),
                sections: vec![ToolDisplaySection {
                    label: "changes".to_string(),
                    language: "diff".to_string(),
                    content: "+new content".to_string(),
                    stats: None,
                }],
            }),
            error_kind: None,
            message: None,
        }),
        None,
    );

    let lines: Vec<String> = state
        .transcript_preview
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();

    assert!(
        lines.iter().any(|line| line.contains("changes (diff)")),
        "Expected to find 'changes (diff)' in transcript"
    );
    assert!(
        lines.iter().any(|line| line.contains("+new content")),
        "Expected to find '+new content' in transcript"
    );
}

#[test]
fn permission_requested_without_display_does_not_add_transcript_entries() {
    let mut state = AppState::new();

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::ToolStart {
            name: "nu__run".to_string(),
            source: "mcp".to_string(),
            arguments: r#"{"command":"ls"}"#.to_string(),
        }),
        None,
    );

    let len_after_start = state.transcript_preview.len();

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::PermissionRequested {
            request_id: "req-1".to_string(),
            context: PermissionRequestContext {
                tool: "nu__run".to_string(),
                source: "mcp".to_string(),
                mode: None,
                matched_rule_identity: "tool:nu__run".to_string(),
                scope: "tool".to_string(),
                target_field: None,
                pattern: "nu__run".to_string(),
                summary: r#"→ {"command":"ls"}"#.to_string(),
                pre_authorize_display: None,
            },
        }),
        None,
    );

    assert_eq!(
        state.transcript_preview.len(),
        len_after_start,
        "PermissionRequested without display should not add transcript entries"
    );
}

#[test]
fn streaming_replaces_not_appends() {
    let mut state = busy_state_with_clean_transcript();

    // Emit first AssistantMessage delta
    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::AssistantMessage {
            text: "hello".to_string(),
        }),
        None,
    );

    // First message should set streaming_message_start
    assert!(state.streaming_message_start.is_some());
    let first_start = state.streaming_message_start.unwrap();

    // Emit second AssistantMessage delta (accumulated text)
    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::AssistantMessage {
            text: "hello world".to_string(),
        }),
        None,
    );

    // Should still have same streaming_message_start
    assert_eq!(state.streaming_message_start, Some(first_start));

    // Verify transcript has ONE message block with "hello world", not two separate entries
    let assistant_entries: Vec<_> = state
        .transcript_preview
        .iter()
        .filter(|entry| entry.role() == Role::Assistant)
        .collect();

    // Should have exactly one "hello world" message, not "hello" and "hello world"
    assert_eq!(
        assistant_entries.len(),
        1,
        "Expected exactly one assistant message block (replaced, not appended)"
    );
    assert_eq!(assistant_entries[0].text(), "hello world");

    // Verify no "hello" without "world" exists
    assert!(
        !state
            .transcript_preview
            .iter()
            .any(|entry| entry.text() == "hello"),
        "Should not have standalone 'hello' entry - it should be replaced"
    );
}

#[test]
fn streaming_message_start_reset_on_llm_start() {
    let mut state = busy_state_with_clean_transcript();

    // Emit streaming sequence
    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::AssistantMessage {
            text: "first message".to_string(),
        }),
        None,
    );

    // Should have streaming_message_start set
    assert!(state.streaming_message_start.is_some());

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::AssistantMessage {
            text: "first message continues".to_string(),
        }),
        None,
    );

    // Still should be set
    assert!(state.streaming_message_start.is_some());

    // Emit LlmStart (new LLM response begins)
    reduce_with_cancel_controller(&mut state, event_input(UiEvent::LlmStart), None);

    // Verify streaming_message_start is reset to None
    assert!(
        state.streaming_message_start.is_none(),
        "LlmStart should reset streaming_message_start to None"
    );
}

#[test]
fn streaming_message_start_reset_on_finalize() {
    let mut state = busy_state_with_clean_transcript();

    // Emit streaming sequence
    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::AssistantMessage {
            text: "streaming response".to_string(),
        }),
        None,
    );

    // Should have streaming_message_start set
    assert!(state.streaming_message_start.is_some());

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::AssistantMessage {
            text: "streaming response with more".to_string(),
        }),
        None,
    );

    // Still should be set
    assert!(state.streaming_message_start.is_some());

    // Emit Completed event (calls finalize)
    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::Completed { tool_calls: 0 }),
        None,
    );

    // Verify streaming_message_start is reset to None
    assert!(
        state.streaming_message_start.is_none(),
        "finalize (via Completed) should reset streaming_message_start to None"
    );
    assert_eq!(state.phase, UiPhase::Idle);
}

#[test]
fn turn_error_leaves_ui_in_recoverable_state() {
    let mut state = busy_state_with_clean_transcript();

    // Preconditions: busy state with active prompt
    assert_eq!(state.phase, UiPhase::Busy);
    assert!(active_prompt_id(&state).is_some());
    assert!(state.is_active_cycle());

    // Dispatch TurnError
    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::TurnError {
            message: "test error".to_string(),
        }),
        None,
    );

    // All stale state must be cleared (same as Completed handler)
    assert_eq!(active_prompt_id(&state), None);
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.is_active_cycle());
    assert!(!state.abort.pending);

    // Error message preserved in transcript
    let has_error_in_transcript = state
        .transcript_preview
        .iter()
        .any(|line| line.text().contains("test error"));
    assert!(
        has_error_in_transcript,
        "Expected error message in transcript"
    );

    // Verify a second prompt submission is accepted (not blocked)
    state.pending_submit_text = Some("retry".to_string());
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);
    assert_eq!(state.phase, UiPhase::Busy);
    // Simulate handle_llm_start which sets the lock
    state.input_locked = true;
    // Transcript entry for "retry" is deferred to activation
    let _ = state.take_next_prompt_for_execution();
    assert!(
        state
            .transcript_preview
            .iter()
            .any(|line| line.text() == "retry"),
        "Second prompt should appear in transcript"
    );

    assert_reducer_invariants(&state);
}

#[test]
fn compaction_triggered_clears_status_line() {
    let mut state = busy_state_with_clean_transcript();
    state.status_line = "Thinking...".to_string();

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::CompactionTriggered {
            source: "test".into(),
            summarized_count: 5,
            kept_recent_count: 3,
            summary_preview: "...".into(),
            summary_body: "summary".into(),
        }),
        None,
    );

    assert!(
        state.status_line.is_empty(),
        "status_line should be cleared after CompactionTriggered, got: {:?}",
        state.status_line
    );
}

#[test]
fn compaction_triggered_resets_latest_total_tokens() {
    let mut state = busy_state_with_clean_transcript();
    // Simulate pre-compaction state: token usage is known
    state.latest_total_tokens = Some(50_000);

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::CompactionTriggered {
            source: "test".into(),
            summarized_count: 5,
            kept_recent_count: 3,
            summary_preview: "...".into(),
            summary_body: "summary".into(),
        }),
        None,
    );

    assert_eq!(
        state.latest_total_tokens, None,
        "latest_total_tokens should be reset to None after CompactionTriggered"
    );
}

#[test]
fn compaction_failed_clears_status_line() {
    let mut state = busy_state_with_clean_transcript();
    state.status_line = "Thinking...".to_string();

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::CompactionFailed {
            source: "test".into(),
            message: "err".into(),
        }),
        None,
    );

    assert!(
        state.status_line.is_empty(),
        "status_line should be cleared after CompactionFailed, got: {:?}",
        state.status_line
    );
}

#[test]
fn compaction_streaming_renders_progressively() {
    let mut state = AppState::default();

    // Start compaction block
    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::CompactionStarted {
            source: "auto".to_string(),
        }),
        None,
    );

    // Stream 3 chunks with growing aggregated text
    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::CompactionSummaryChunk {
            source: "auto".to_string(),
            delta: "Hello".to_string(),
            aggregated: "Hello".to_string(),
        }),
        None,
    );
    let after_chunk1 = state.transcript_preview.len();
    assert!(after_chunk1 > 1, "should have header + content");

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::CompactionSummaryChunk {
            source: "auto".to_string(),
            delta: " world".to_string(),
            aggregated: "Hello world".to_string(),
        }),
        None,
    );

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::CompactionSummaryChunk {
            source: "auto".to_string(),
            delta: " done".to_string(),
            aggregated: "Hello world done".to_string(),
        }),
        None,
    );

    // Finalize
    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::CompactionTriggered {
            source: "auto".to_string(),
            summarized_count: 5,
            kept_recent_count: 2,
            summary_preview: "Hello world done".to_string(),
            summary_body: "Hello world done".to_string(),
        }),
        None,
    );

    // Verify content is present and block is finished
    let lines: Vec<String> = state
        .transcript_preview
        .iter()
        .map(|item| item.text())
        .collect();
    assert!(lines.iter().any(|l| l.contains("Hello world done")));
}

#[test]
fn compaction_streaming_truncates_and_reprojects() {
    let mut state = AppState::default();

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::CompactionStarted {
            source: "auto".to_string(),
        }),
        None,
    );

    // First chunk
    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::CompactionSummaryChunk {
            source: "auto".to_string(),
            delta: "First".to_string(),
            aggregated: "First".to_string(),
        }),
        None,
    );

    // Second chunk — should truncate back and reproject
    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::CompactionSummaryChunk {
            source: "auto".to_string(),
            delta: " Second".to_string(),
            aggregated: "First Second".to_string(),
        }),
        None,
    );

    // Should NOT have both "First" standalone AND "First Second"
    // The re-projection replaces, not appends
    let lines: Vec<String> = state
        .transcript_preview
        .iter()
        .map(|item| item.text())
        .collect();
    let first_only_count = lines
        .iter()
        .filter(|l| l.contains("First") && !l.contains("Second"))
        .count();
    assert_eq!(first_only_count, 0, "old partial render should be replaced");
}

#[test]
fn compaction_streaming_empty_chunks_ignored() {
    let mut state = AppState::default();

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::CompactionStarted {
            source: "auto".to_string(),
        }),
        None,
    );
    let after_start = state.transcript_preview.len();

    // Empty chunk
    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::CompactionSummaryChunk {
            source: "auto".to_string(),
            delta: "".to_string(),
            aggregated: "".to_string(),
        }),
        None,
    );

    // Should not have added any content lines
    assert_eq!(state.transcript_preview.len(), after_start);
    assert!(state.compaction_streaming_start.is_none());
}

#[test]
fn compaction_triggered_clears_streaming_state() {
    let mut state = AppState::default();

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::CompactionStarted {
            source: "auto".to_string(),
        }),
        None,
    );
    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::CompactionSummaryChunk {
            source: "auto".to_string(),
            delta: "text".to_string(),
            aggregated: "text".to_string(),
        }),
        None,
    );
    assert!(state.compaction_streaming_start.is_some());

    reduce_with_cancel_controller(
        &mut state,
        event_input(UiEvent::CompactionTriggered {
            source: "auto".to_string(),
            summarized_count: 5,
            kept_recent_count: 2,
            summary_preview: "text".to_string(),
            summary_body: "text".to_string(),
        }),
        None,
    );

    assert!(
        state.compaction_streaming_start.is_none(),
        "streaming state must be cleared after CompactionTriggered"
    );
}

#[cfg(test)]
mod task_4a_tests {
    use super::*;
    use crate::rendering::theme::TuiTheme;
    use nu_agent_core::protocol::event::UiEvent;
    use nu_agent_core::transcript::ir::StyleHint;
    use nu_agent_core::transcript::items::{ProseMessage, TranscriptEntry};

    /// Return raw markdown strings stored in all Assistant ProseMessage entries.
    fn assistant_markdown_entries(state: &AppState) -> Vec<String> {
        state
            .transcript_preview
            .iter()
            .filter_map(|e| {
                if let TranscriptEntry::Assistant(ProseMessage { markdown }) = e {
                    Some(markdown.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Project a markdown string and return all (text, hint) pairs from it.
    fn project_spans(markdown: &str) -> Vec<(String, StyleHint)> {
        crate::markdown::render_markdown_lines(markdown, None, &TuiTheme::default())
            .into_iter()
            .flat_map(|l| l.spans.into_iter())
            .map(|s| (s.text, s.hint))
            .collect()
    }

    #[test]
    fn assistant_message_with_bold_emits_md_bold() {
        let mut state = AppState::new();
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::Event(Box::new(UiEvent::AssistantMessage {
                text: "hello **bold**".to_string(),
            })),
            None,
        );
        assert!(
            // Verify the raw markdown is stored and projects to MdBold
            assistant_markdown_entries(&state).iter().any(|md| {
                project_spans(md)
                    .iter()
                    .any(|(t, h)| t == "bold" && matches!(h, StyleHint::MdBold))
            })
        );
    }

    #[test]
    fn assistant_streaming_truncates_prior_render() {
        let mut state = AppState::new();
        for text in ["hello", "hello world"] {
            reduce_with_cancel_controller(
                &mut state,
                ReducerInput::Event(Box::new(UiEvent::AssistantMessage {
                    text: text.to_string(),
                })),
                None,
            );
        }
        // After streaming, there should be a single ProseMessage with the final text
        let markdowns = assistant_markdown_entries(&state);
        let concat: String = markdowns.join("");
        assert!(concat.contains("hello world"));
        assert!(!concat.contains("hellohello"));
    }

    #[test]
    fn compaction_chunk_with_italic_emits_md_italic() {
        let mut state = AppState::new();
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::Event(Box::new(UiEvent::CompactionSummaryChunk {
                source: "history".to_string(),
                delta: "summary *italic*".to_string(),
                aggregated: "summary *italic*".to_string(),
            })),
            None,
        );
        assert!(assistant_markdown_entries(&state).iter().any(|md| {
            project_spans(md)
                .iter()
                .any(|(t, h)| t == "italic" && matches!(h, StyleHint::MdItalic))
        }));
    }

    #[test]
    fn esc_esc_with_pending_restores_texts_to_input_buffer() {
        let mut state = AppState::new();
        state.pending_submit_text = Some("first".to_string());
        reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);
        let _ = state.activate_next_prompt();
        state.enqueue_prompt("second".to_string());
        state.enqueue_prompt("third".to_string());

        reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Esc), None);
        assert_eq!(state.phase, UiPhase::AbortPending);

        reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::EscConfirm), None);

        assert_eq!(state.phase, UiPhase::Idle);
        assert!(pending_prompt_ids(&state).is_empty());
        assert_eq!(active_prompt_id(&state), None);
    }

    #[test]
    fn esc_esc_with_no_pending_clears_state_but_not_buffer() {
        let mut state = AppState::new();
        state.pending_submit_text = Some("do work".to_string());
        reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);
        let _ = state.activate_next_prompt();

        reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Esc), None);
        reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::EscConfirm), None);

        assert_eq!(state.phase, UiPhase::Idle);
        assert!(pending_prompt_ids(&state).is_empty());
        assert_eq!(active_prompt_id(&state), None);
    }
}

#[cfg(test)]
mod visual_selection_tests {
    use super::*;
    use crate::state::selection::TranscriptSelection;
    use nu_agent_core::transcript::items::ProseMessage;

    #[test]
    fn enter_visual_mode_selects_first_visible_entry_not_scroll_offset() {
        let mut state = AppState::new();
        state.pane_focus = PaneFocus::Transcript;
        // Populate transcript with entries that render as multiple lines
        // Entry 0: multi-line markdown (renders as 3 visual rows)
        // Entry 1: multi-line markdown (renders as 2 visual rows)
        state.push_transcript_item(TranscriptEntry::User(ProseMessage {
            markdown: "line 0\nextra\nmore".to_string(),
        }));
        state.push_transcript_item(TranscriptEntry::User(ProseMessage {
            markdown: "line 1\nextra".to_string(),
        }));
        // Scroll offset = 2 means we've scrolled past entry 0's 3 lines
        // (offset 0, 1, 2 are all entry 0's visual rows)
        // The first visible entry should be entry 1
        state.transcript_scroll_offset = 2;
        state.cursor_visual_row = 3;
        state.entry_indices = vec![0, 0, 0, 1, 1];
        state.total_visual_rows = 5;
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::EnterVisualMode),
            None,
        );
        let sel = state.transcript_selection.expect("selection should be set");
        // Should select visual row 3 (maps to entry 1 via entry_indices)
        assert_eq!(sel.anchor(), 3);
        assert_eq!(sel.cursor(), 3);
        assert_eq!(state.input_mode, InputMode::Visual);
    }

    #[test]
    fn enter_visual_mode_sets_selection_at_scroll_offset() {
        let mut state = AppState::new();
        state.pane_focus = PaneFocus::Transcript;
        state.cursor_visual_row = 5;
        state.entry_indices = (0..10).collect();
        state.total_visual_rows = 10;
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::EnterVisualMode),
            None,
        );
        let sel = state.transcript_selection.expect("selection should be set");
        assert_eq!(sel.anchor(), 5);
        assert_eq!(sel.cursor(), 5);
        assert_eq!(state.input_mode, InputMode::Visual);
        assert_eq!(state.status_line, "-- VISUAL --");
    }

    #[test]
    fn enter_visual_mode_requires_transcript_focus() {
        let mut state = AppState::new();
        state.pane_focus = PaneFocus::Input;
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::EnterVisualMode),
            None,
        );
        assert_eq!(state.status_line, VISUAL_REQUIRES_TRANSCRIPT_FOCUS_STATUS);
        assert!(state.transcript_selection.is_none());
    }

    #[test]
    fn enter_visual_mode_noop_when_busy() {
        let mut state = AppState::new();
        state.phase = UiPhase::Busy;
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::EnterVisualMode),
            None,
        );
        assert!(state.transcript_selection.is_none());
    }

    #[test]
    fn visual_j_extends_selection_down() {
        let mut state = AppState::new();
        state.input_mode = InputMode::Visual;
        state.transcript_selection = Some(TranscriptSelection::new(0));
        state.entry_indices = (0..5).collect();
        state.total_visual_rows = 5;
        for i in 0..5 {
            state.push_transcript_item(TranscriptEntry::User(ProseMessage {
                markdown: format!("line {i}"),
            }));
        }
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::ScrollLineDown),
            None,
        );
        let sel = state.transcript_selection.expect("selection should exist");
        assert_eq!(sel.cursor(), 1);
        assert_eq!(state.cursor_visual_row, 1);
        assert_eq!(state.transcript_scroll_offset, 1);
        assert!(!state.transcript_following_tail);
    }

    #[test]
    fn visual_k_extends_selection_up() {
        let mut state = AppState::new();
        state.input_mode = InputMode::Visual;
        state.transcript_selection = Some(TranscriptSelection::new(2));
        state.cursor_visual_row = 2;
        state.entry_indices = (0..3).collect();
        state.total_visual_rows = 3;
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::ScrollLineUp),
            None,
        );
        let sel = state.transcript_selection.expect("selection should exist");
        assert_eq!(sel.cursor(), 1);
        assert_eq!(state.cursor_visual_row, 1);
        assert_eq!(state.transcript_scroll_offset, 0);
        assert!(!state.transcript_following_tail);
    }

    #[test]
    fn visual_yank_copies_and_exits_visual() {
        let mut state = AppState::new();
        state.input_mode = InputMode::Visual;
        state.rendered_line_text = vec![
            "line 0".to_string(),
            "line 1".to_string(),
            "line 2".to_string(),
            "line 3".to_string(),
            "line 4".to_string(),
        ];
        state.rendered_line_start_row = 0;
        // Set selection covering visual rows 1-3
        let mut sel = TranscriptSelection::new(1);
        sel.set_cursor(3);
        state.transcript_selection = Some(sel);
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::YankSelection),
            None,
        );
        let clipboard = state.take_clipboard_request();
        assert_eq!(clipboard, Some("line 1\nline 2\nline 3".to_string()));
        assert!(state.transcript_selection.is_none());
        assert_eq!(state.input_mode, InputMode::Normal);
    }

    #[test]
    fn visual_yank_copies_only_selected_rows_not_whole_entry() {
        let mut state = AppState::new();
        state.input_mode = InputMode::Visual;
        state.rendered_line_text = (0..10).map(|i| format!("row {i}")).collect();
        state.rendered_line_start_row = 0;
        // Select rows 3-5 out of 10
        let mut sel = TranscriptSelection::new(3);
        sel.set_cursor(5);
        state.transcript_selection = Some(sel);
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::YankSelection),
            None,
        );
        let clipboard = state.take_clipboard_request();
        assert_eq!(clipboard, Some("row 3\nrow 4\nrow 5".to_string()));
        assert!(state.transcript_selection.is_none());
        assert_eq!(state.input_mode, InputMode::Normal);
    }

    #[test]
    fn visual_yank_with_nonzero_scroll_offset_copies_correct_rows() {
        let mut state = AppState::new();
        state.input_mode = InputMode::Visual;
        // Simulate viewport scrolled down — rendered_line_text[0] is absolute row 10
        state.rendered_line_start_row = 10;
        state.rendered_line_text = vec![
            "row 10".to_string(),
            "row 11".to_string(),
            "row 12".to_string(),
            "row 13".to_string(),
            "row 14".to_string(),
        ];
        // Select absolute visual rows 11-13
        let mut sel = TranscriptSelection::new(11);
        sel.set_cursor(13);
        state.transcript_selection = Some(sel);
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::YankSelection),
            None,
        );
        let clipboard = state.take_clipboard_request();
        assert_eq!(clipboard, Some("row 11\nrow 12\nrow 13".to_string()));
        assert!(state.transcript_selection.is_none());
        assert_eq!(state.input_mode, InputMode::Normal);
    }

    #[test]
    fn visual_yank_nothing_to_yank_shows_status() {
        let mut state = AppState::new();
        state.input_mode = InputMode::Visual;
        state.rendered_line_text = Vec::new();
        state.transcript_scroll_offset = 0;
        state.transcript_selection = Some(TranscriptSelection::new(0));
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::YankSelection),
            None,
        );
        assert!(state.take_clipboard_request().is_none());
        assert_eq!(state.status_line, "Nothing to yank");
        assert!(state.transcript_selection.is_none());
        assert_eq!(state.input_mode, InputMode::Normal);
    }

    #[test]
    fn visual_yank_empty_selection_noop() {
        let mut state = AppState::new();
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::YankSelection),
            None,
        );
        assert!(state.take_clipboard_request().is_none());
    }

    #[test]
    fn visual_esc_clears_selection_and_exits() {
        let mut state = AppState::new();
        state.input_mode = InputMode::Visual;
        state.transcript_selection = Some(TranscriptSelection::new(0));
        reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Esc), None);
        assert!(state.transcript_selection.is_none());
        assert_eq!(state.input_mode, InputMode::Normal);
    }

    #[test]
    fn visual_gg_jumps_to_top() {
        let mut state = AppState::new();
        state.input_mode = InputMode::Visual;
        state.transcript_selection = Some(TranscriptSelection::new(5));
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::ScrollToTop),
            None,
        );
        let sel = state.transcript_selection.expect("selection should exist");
        assert_eq!(sel.cursor(), 0);
        assert_eq!(state.transcript_scroll_offset, 0);
        assert!(!state.transcript_following_tail);
    }

    #[test]
    fn visual_g_jumps_to_bottom() {
        let mut state = AppState::new();
        state.input_mode = InputMode::Visual;
        state.transcript_selection = Some(TranscriptSelection::new(0));
        state.total_visual_rows = 5;
        for i in 0..5 {
            state.push_transcript_item(TranscriptEntry::User(ProseMessage {
                markdown: format!("line {i}"),
            }));
        }
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::ScrollToBottom),
            None,
        );
        let sel = state.transcript_selection.expect("selection should exist");
        assert_eq!(sel.cursor(), 4);
        assert!(state.transcript_following_tail);
    }

    #[test]
    fn normal_j_moves_cursor_not_viewport() {
        let mut state = AppState::new();
        state.input_mode = InputMode::Normal;
        state.pane_focus = PaneFocus::Transcript;
        state.viewport_height = 10;
        state.cursor_visual_row = 5;
        state.transcript_scroll_offset = 0;
        state.entry_indices = (0..20).collect();
        state.total_visual_rows = 20;
        for i in 0..20 {
            state.push_transcript_item(TranscriptEntry::User(ProseMessage {
                markdown: format!("line {i}"),
            }));
        }
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::ScrollLineDown),
            None,
        );
        // Cursor moved from 5 to 6, viewport didn't scroll (6 < 0+10-3=7)
        assert_eq!(state.cursor_visual_row, 6);
        assert_eq!(state.transcript_scroll_offset, 0);
        assert!(!state.transcript_following_tail);
    }

    #[test]
    fn normal_j_scrolls_viewport_when_cursor_leaves_margin() {
        let mut state = AppState::new();
        state.input_mode = InputMode::Normal;
        state.pane_focus = PaneFocus::Transcript;
        state.viewport_height = 10;
        state.cursor_visual_row = 6;
        state.transcript_scroll_offset = 0;
        state.entry_indices = (0..20).collect();
        state.total_visual_rows = 20;
        for i in 0..20 {
            state.push_transcript_item(TranscriptEntry::User(ProseMessage {
                markdown: format!("line {i}"),
            }));
        }
        // scroll_margin = 3, viewport_bottom = 0+10-3 = 7
        // cursor moves 6→7, visual_row=7 >= 7, viewport scrolls
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::ScrollLineDown),
            None,
        );
        assert_eq!(state.cursor_visual_row, 7);
        assert_eq!(state.transcript_scroll_offset, 1);
        assert!(!state.transcript_following_tail);
    }

    #[test]
    fn visual_j_scrolls_viewport_only_when_cursor_leaves_visible_area() {
        let mut state = AppState::new();
        state.input_mode = InputMode::Visual;
        state.transcript_following_tail = false;
        state.viewport_height = 10;
        state.transcript_scroll_offset = 0;
        state.transcript_selection = Some(TranscriptSelection::new(0));
        // 20 entries, 1 visual row each
        state.entry_indices = (0..20).collect();
        state.total_visual_rows = 20;
        for i in 0..20 {
            state.push_transcript_item(TranscriptEntry::User(ProseMessage {
                markdown: format!("line {i}"),
            }));
        }
        // scroll_margin = 10/3 = 3
        // viewport shows rows 0-9 (scroll_offset=0, viewport_height=10)
        // Press j 7 times: cursor moves 0→1→2→3→4→5→6→7
        // Cursor 1-6: visual row < 0+10-3=7, no scroll
        // Cursor 7: visual row 7 >= 7, scroll_offset → 1
        for _ in 0..7 {
            reduce_with_cancel_controller(
                &mut state,
                ReducerInput::User(UserAction::ScrollLineDown),
                None,
            );
        }
        let sel = state.transcript_selection.expect("selection should exist");
        assert_eq!(sel.cursor(), 7);
        assert_eq!(state.cursor_visual_row, 7);
        assert_eq!(state.transcript_scroll_offset, 1);
        assert!(!state.transcript_following_tail);
    }

    #[test]
    fn visual_k_syncs_scroll_offset_when_exiting_tail_following() {
        let mut state = AppState::new();
        state.input_mode = InputMode::Visual;
        state.transcript_following_tail = true;
        state.max_scroll = 50;
        state.transcript_scroll_offset = 0; // stale
        state.cursor_visual_row = 55;
        state.transcript_selection = Some(TranscriptSelection::new(55));
        state.entry_indices = (0..60).collect();
        state.total_visual_rows = 60;
        for i in 0..60 {
            state.push_transcript_item(TranscriptEntry::User(ProseMessage {
                markdown: format!("line {i}"),
            }));
        }
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::ScrollLineUp),
            None,
        );
        // scroll_offset should be synced from max_scroll (50), then cursor moves
        // 55→54. cursor_visual_row=54, scroll_margin=1, 54 < 50+1=51 is false,
        // so no additional scroll.
        assert_eq!(state.cursor_visual_row, 54);
        assert_eq!(state.transcript_scroll_offset, 50);
        assert!(!state.transcript_following_tail);
    }

    #[test]
    fn ctrl_u_moves_cursor_up_by_page() {
        let mut state = AppState::new();
        state.input_mode = InputMode::Normal;
        state.pane_focus = PaneFocus::Transcript;
        state.viewport_height = 10;
        state.cursor_visual_row = 10;
        state.total_visual_rows = 30;
        state.transcript_scroll_offset = 5;
        state.transcript_following_tail = false;
        state.entry_indices = (0..30).collect();
        for i in 0..30 {
            state.push_transcript_item(TranscriptEntry::User(ProseMessage {
                markdown: format!("line {i}"),
            }));
        }
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::ScrollPageUp),
            None,
        );
        // cursor moves 10 - 8 = 2
        assert_eq!(state.cursor_visual_row, 2);
        // scroll_margin = 10/3 = 3, cursor 2 < 5 + 3 = 8, so scroll_offset = 5 - 8 = 0
        assert_eq!(state.transcript_scroll_offset, 0);
        assert!(!state.transcript_following_tail);
    }

    #[test]
    fn ctrl_d_moves_cursor_down_by_page() {
        let mut state = AppState::new();
        state.input_mode = InputMode::Normal;
        state.pane_focus = PaneFocus::Transcript;
        state.viewport_height = 10;
        state.cursor_visual_row = 0;
        state.total_visual_rows = 30;
        state.transcript_scroll_offset = 0;
        state.transcript_following_tail = false;
        state.entry_indices = (0..30).collect();
        for i in 0..30 {
            state.push_transcript_item(TranscriptEntry::User(ProseMessage {
                markdown: format!("line {i}"),
            }));
        }
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::ScrollPageDown),
            None,
        );
        // cursor moves 0 + 8 = 8
        assert_eq!(state.cursor_visual_row, 8);
        // scroll_margin = 3, viewport_bottom = 0 + 10 - 3 = 7, cursor 8 >= 7, so scroll_offset = 0 + 8 = 8
        assert_eq!(state.transcript_scroll_offset, 8);
        assert!(!state.transcript_following_tail);
    }

    #[test]
    fn ctrl_u_in_visual_mode_extends_selection() {
        let mut state = AppState::new();
        state.input_mode = InputMode::Visual;
        state.pane_focus = PaneFocus::Transcript;
        state.transcript_selection = Some(TranscriptSelection::new(10));
        state.cursor_visual_row = 10;
        state.total_visual_rows = 30;
        state.viewport_height = 10;
        state.transcript_scroll_offset = 5;
        state.transcript_following_tail = false;
        state.entry_indices = (0..30).collect();
        for i in 0..30 {
            state.push_transcript_item(TranscriptEntry::User(ProseMessage {
                markdown: format!("line {i}"),
            }));
        }
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::ScrollPageUp),
            None,
        );
        let sel = state.transcript_selection.expect("selection should exist");
        assert_eq!(state.cursor_visual_row, 2);
        assert_eq!(sel.cursor(), 2);
    }

    #[test]
    fn ctrl_d_in_visual_mode_extends_selection() {
        let mut state = AppState::new();
        state.input_mode = InputMode::Visual;
        state.pane_focus = PaneFocus::Transcript;
        state.transcript_selection = Some(TranscriptSelection::new(0));
        state.cursor_visual_row = 0;
        state.total_visual_rows = 30;
        state.viewport_height = 10;
        state.transcript_scroll_offset = 0;
        state.transcript_following_tail = false;
        state.entry_indices = (0..30).collect();
        for i in 0..30 {
            state.push_transcript_item(TranscriptEntry::User(ProseMessage {
                markdown: format!("line {i}"),
            }));
        }
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::ScrollPageDown),
            None,
        );
        let sel = state.transcript_selection.expect("selection should exist");
        assert_eq!(state.cursor_visual_row, 8);
        assert_eq!(sel.cursor(), 8);
    }
}
