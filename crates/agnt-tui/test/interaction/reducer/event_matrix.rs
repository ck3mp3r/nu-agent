use super::*;

use agnt_core::protocol::events::{RequestId, SystemKind, ToolCall, ToolOutcome, Usage};
use agnt_core::session::tool_status::ToolStatus;
use agnt_core::transcript::ir::ToolName;

#[test]
fn table_driven_ui_event_matrix_covers_all_variants() {
    struct Case {
        name: &'static str,
        event: SystemEvent,
        pre: fn() -> AppState,
    }

    fn idle() -> AppState {
        AppState::default()
    }

    fn busy_empty_status() -> AppState {
        let mut state = busy_state_with_clean_transcript();
        state.status.message.clear();
        state
    }

    fn busy_with_status() -> AppState {
        let mut state = busy_state_with_clean_transcript();
        state.status.message.set_message("Tool: prior");
        state
    }

    fn busy_with_running_tool_line() -> AppState {
        let mut state = busy_state_with_clean_transcript();
        reduce_ui(
            &mut state,
            SystemEvent::ToolStarted {
                call: ToolCall::new(
                    ToolName("k8s__list_pods".to_string()),
                    "mcp",
                    r#"{"namespace":"prod"}"#,
                    CallLine::from_json_summary(r#"{"namespace":"prod"}"#),
                ),
            },
        );
        state
    }

    let cases = vec![
        Case {
            name: "llm_start_from_idle_moves_busy",
            event: SystemEvent::InferenceStarted,
            pre: idle,
        },
        Case {
            name: "llm_start_when_busy_is_noop",
            event: SystemEvent::InferenceStarted,
            pre: busy_with_status,
        },
        Case {
            name: "tick_leaves_empty_status_empty",
            event: SystemEvent::Tick,
            pre: busy_empty_status,
        },
        Case {
            name: "tick_preserves_existing_status",
            event: SystemEvent::Tick,
            pre: busy_with_status,
        },
        Case {
            name: "tool_start_leaves_status_empty",
            event: SystemEvent::ToolStarted {
                call: ToolCall::new(
                    ToolName("k8s__list_pods".to_string()),
                    "mcp",
                    "{}",
                    CallLine::from_json_summary("{}"),
                ),
            },
            pre: busy_empty_status,
        },
        Case {
            name: "tool_end_updates_tool_line_without_status",
            event: SystemEvent::ToolCompleted {
                call: ToolCall::new(
                    ToolName("k8s__list_pods".to_string()),
                    "mcp",
                    r#"{"namespace":"prod"}"#,
                    CallLine::from_json_summary(r#"{"namespace":"prod"}"#),
                ),
                outcome: ToolOutcome {
                    success: true,
                    status: ToolStatus::Success,
                    result: "[]".to_string(),
                    display: None,
                    error_kind: None,
                    message: None,
                },
            },
            pre: busy_with_running_tool_line,
        },
        Case {
            name: "llm_end_records_tokens_without_status_message",
            event: SystemEvent::InferenceCompleted {
                usage: Usage {
                    response_chars: 12,
                    tool_calls: 0,
                    input_tokens: 4,
                    output_tokens: 8,
                    total_tokens: Some(12),
                    cost: None,
                    cost_complete: false,
                },
            },
            pre: busy_empty_status,
        },
        Case {
            name: "warning_sets_status_line",
            event: SystemEvent::SystemMessage {
                kind: SystemKind::Warning,
                text: "warned".to_string(),
            },
            pre: busy_empty_status,
        },
        Case {
            name: "assistant_message_trims_and_appends",
            event: SystemEvent::InferenceChunk {
                delta: "\nline 1\nline 2\n".to_string(),
                aggregated: "\nline 1\nline 2\n".to_string(),
            },
            pre: busy_empty_status,
        },
        Case {
            name: "completed_finalizes_cycle",
            event: SystemEvent::TurnCompleted {
                tool_calls: 1,
                input_tokens: 0,
                output_tokens: 0,
                total_tokens: None,
                cost: None,
                cost_complete: true,
                duration_ms: 0,
            },
            pre: busy_with_status,
        },
    ];

    for case in cases {
        let mut state = (case.pre)();
        reduce_ui(&mut state, case.event);

        match case.name {
            "llm_start_from_idle_moves_busy" => {
                assert_eq!(state.phase, UiPhase::Busy);
                assert!(state.input_locked);
            }
            "llm_start_when_busy_is_noop" => {
                assert_eq!(state.phase, UiPhase::Busy);
                assert_eq!(state.status.message.status_line(), "Tool: prior");
                assert!(state.input_locked);
            }
            "tick_leaves_empty_status_empty" => {
                assert!(state.status.message.status_line().is_empty());
            }
            "tick_preserves_existing_status" => {
                assert_eq!(state.status.message.status_line(), "Tool: prior");
            }
            "tool_start_leaves_status_empty" => {
                assert!(state.status.message.status_line().is_empty());
            }
            "tool_end_updates_tool_line_without_status" => {
                // [Tool] — no leading spacer under the unified spacer rule
                assert_eq!(state.transcript.len(), 1);
                assert!(matches!(
                    &state.transcript.entries()[0],
                    TranscriptEntry::Tool(_)
                ));
                assert!(state.status.message.status_line().is_empty());
            }
            "llm_end_records_tokens_without_status_message" => {
                assert_eq!(state.status.tokens.latest_input_tokens, Some(4));
                assert_eq!(state.status.tokens.latest_output_tokens, Some(8));
                assert_eq!(state.status.tokens.latest_total_tokens, Some(12));
                assert_eq!(state.status.tokens.session_total_tokens, 12);
                assert!(state.status.message.status_line().is_empty());
            }
            "warning_sets_status_line" => {
                // Warning is now handled by the SystemEvent dispatch, which routes
                // to StatusState::reduce_warning_event and sets the status line.
                assert_eq!(state.status.message.status_line(), "warned");
            }
            "assistant_message_trims_and_appends" => {
                // After the raw-markdown refactor, a single AssistantMessage
                // produces one message entry. The raw text is trimmed before
                // storage, so leading/trailing whitespace is dropped.
                let assistant_entries: Vec<String> = state
                    .transcript
                    .entries()
                    .iter()
                    .filter(|entry| {
                        matches!(
                            entry,
                            TranscriptEntry::Message(message)
                                if message.role == MessageRole::Assistant
                        )
                    })
                    .map(entry_text)
                    .collect();
                assert_eq!(assistant_entries.len(), 1, "one assistant entry");
                let text = &assistant_entries[0];
                assert!(text.contains("line 1"), "raw md should contain 'line 1'");
                assert!(text.contains("line 2"), "raw md should contain 'line 2'");
            }
            "completed_finalizes_cycle" => {
                assert_eq!(state.phase, UiPhase::Idle);
                assert!(!state.input_locked);
                assert!(!state.abort.pending);
                assert!(state.status.message.status_line().is_empty());
            }
            _ => unreachable!("unknown case: {}", case.name),
        }

        assert_reducer_invariants(&state);
    }
}

#[test]
fn permission_request_table() -> Result<()> {
    // A permission request focuses the transcript for immediate prompt visibility.
    let mut state = AppState::default();
    state.scroll.pane_focus = crate::state::PaneFocus::Input;

    let context = PermissionRequestContext {
        tool: "edit(file=foo.rs)".to_string(),
        tool_key: "edit\n{\"file\":\"foo.rs\"}".to_string(),
        source: "closure".to_string(),
        mode: Some("apply".to_string()),
        matched_rule_identity: "tool:edit".to_string(),
        scope: "tool".to_string(),
        target_field: None,
        pattern: "edit".to_string(),
        summary: "→ {...}".to_string(),
        pre_authorize_display: None,
    };
    reduce_ui(
        &mut state,
        SystemEvent::PermissionRequested {
            request_id: RequestId::new("ask-0000000000000001"),
            context,
        },
    );

    assert_eq!(state.scroll.pane_focus, crate::state::PaneFocus::Input);
    assert!(state.permission.has_prompt());

    // PermissionRequested dispatch orders the tool before the diff preview and follows the tail.
    let mut state = AppState::default();
    state.scroll.following_tail = false;

    reduce_ui(
        &mut state,
        SystemEvent::ToolStarted {
            call: ToolCall::new(
                ToolName("edit".to_string()),
                "builtin",
                "{}",
                CallLine::from_json_summary("{}"),
            ),
        },
    );
    reduce_ui(
        &mut state,
        SystemEvent::PermissionRequested {
            request_id: RequestId::new("perm-1"),
            context: PermissionRequestContext {
                tool: "edit".to_string(),
                tool_key: "edit\n{}".to_string(),
                source: "builtin".to_string(),
                mode: None,
                matched_rule_identity: "tool:edit".to_string(),
                scope: "tool".to_string(),
                target_field: None,
                pattern: "edit".to_string(),
                summary: "test".to_string(),
                pre_authorize_display: Some(ToolDisplay {
                    title: Some("file (diff)".to_string()),
                    sections: vec![ToolDisplaySection {
                        label: Some("diff".to_string()),
                        kind: agnt_core::transcript::ir::ContentKind::Diff {
                            language: "diff".to_string(),
                            path: "test.rs".to_string(),
                        },
                        content: "--- a\n+++ b\n".to_string(),
                        stats: None,
                    }],
                }),
            },
        },
    );

    // The preview lands on the pending tool entry in place, so there is
    // exactly one entry.
    assert_eq!(
        state.transcript.len(),
        1,
        "the preview must live on the tool entry"
    );
    let entry = state
        .transcript
        .entries()
        .first()
        .ok_or("tool entry should exist")?;
    let TranscriptEntry::Tool(tool) = entry else {
        return Err("expected Tool entry".into());
    };
    assert!(
        tool.preview.is_some(),
        "the permission request display must land on the entry's preview"
    );
    assert!(
        state.permission.has_prompt(),
        "permission prompt must be open after PermissionRequested"
    );
    assert!(
        state.scroll.following_tail,
        "dispatch must scroll transcript to bottom"
    );
    Ok(())
}

#[test]
fn warning_dispatch_sets_status_line() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec
    reduce_ui(
        &mut state,
        SystemEvent::SystemMessage {
            kind: SystemKind::Warning,
            text: "test warning".to_string(),
        },
    );

    // -- Check
    assert_eq!(
        state.status.message.status_line(),
        "test warning",
        "warning dispatch must set the status line"
    );
    Ok(())
}
