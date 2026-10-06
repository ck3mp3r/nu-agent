use super::*;

#[test]
fn table_driven_ui_event_matrix_covers_all_variants() {
    struct Case {
        name: &'static str,
        event: UiEvent,
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
        reduce_with_cancel_controller(
            &mut state,
            event_input(UiEvent::ToolStarted {
                name: "k8s__list_pods".to_string(),
                source: "mcp".to_string(),
                arguments: r#"{"namespace":"prod"}"#.to_string(),
                call_line: CallLine::from_json_summary(r#"{"namespace":"prod"}"#),
            }),
            None,
        );
        state
    }

    let cases = vec![
        Case {
            name: "llm_start_from_idle_moves_busy",
            event: UiEvent::LlmStarted,
            pre: idle,
        },
        Case {
            name: "llm_start_when_busy_is_noop",
            event: UiEvent::LlmStarted,
            pre: busy_with_status,
        },
        Case {
            name: "tick_leaves_empty_status_empty",
            event: UiEvent::Tick,
            pre: busy_empty_status,
        },
        Case {
            name: "tick_preserves_existing_status",
            event: UiEvent::Tick,
            pre: busy_with_status,
        },
        Case {
            name: "tool_start_leaves_status_empty",
            event: UiEvent::ToolStarted {
                name: "k8s__list_pods".to_string(),
                source: "mcp".to_string(),
                arguments: "{}".to_string(),
                call_line: CallLine::from_json_summary("{}"),
            },
            pre: busy_empty_status,
        },
        Case {
            name: "tool_end_updates_tool_line_without_status",
            event: UiEvent::ToolCompleted {
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
            name: "llm_end_records_tokens_without_status_message",
            event: UiEvent::LlmCompleted {
                response_chars: 12,
                tool_calls: 0,
                input_tokens: 4,
                output_tokens: 8,
                total_tokens: 12,
            },
            pre: busy_empty_status,
        },
        Case {
            name: "warning_sets_status_line",
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
                    state.transcript.blocks()[0].source,
                    BlockSource::Tool { .. }
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
                // Warning is now handled by the UiEvent dispatch, which routes
                // to StatusState::reduce_warning_event and sets the status line.
                assert_eq!(state.status.message.status_line(), "warned");
            }
            "assistant_message_trims_and_appends" => {
                // After the raw-markdown refactor, a single AssistantMessage
                // produces one markdown block. The raw text is trimmed before
                // storage, so leading/trailing whitespace is dropped.
                let assistant_entries: Vec<_> = state
                    .transcript
                    .blocks()
                    .iter()
                    .filter(|b| {
                        matches!(
                            b.source,
                            BlockSource::Markdown {
                                role: nu_agent_core::transcript::ir::MessageRole::Assistant,
                                ..
                            }
                        )
                    })
                    .map(|block| block.source.plain_text())
                    .collect();
                assert_eq!(assistant_entries.len(), 1, "one assistant block");
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
fn permission_request_focuses_transcript_for_immediate_prompt_visibility() {
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
    state
        .permission
        .reduce_permission_event(nu_agent_core::bus::PermissionEvent::Requested {
            request_id: "ask-0000000000000001".to_string(),
            context: Box::new(context),
        });

    assert_eq!(state.scroll.pane_focus, crate::state::PaneFocus::Input);
    assert!(state.permission.has_prompt());
}

#[test]
fn permission_requested_dispatch_orders_tool_before_diff_preview_and_follows_tail() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    state.scroll.following_tail = false;

    // -- Exec
    dispatch_ui_event(
        &mut state,
        UiEvent::ToolStarted {
            name: "edit".to_string(),
            source: "builtin".to_string(),
            arguments: "{}".to_string(),
            call_line: CallLine::from_json_summary("{}"),
        },
    );
    dispatch_ui_event(
        &mut state,
        UiEvent::PermissionRequested {
            request_id: "perm-1".to_string(),
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
                    title: "file (diff)".to_string(),
                    sections: vec![ToolDisplaySection {
                        label: "diff".to_string(),
                        kind: nu_agent_core::transcript::ir::ContentKind::Diff {
                            language: "diff".to_string(),
                        },
                        content: "--- a\n+++ b\n".to_string(),
                        stats: None,
                    }],
                }),
            },
        },
    );

    // -- Check
    // The preview is its own ToolDisplay block pushed directly after the
    // pending Tool block, so there are exactly two blocks.
    assert_eq!(
        state.transcript.len(),
        2,
        "preview must push its own ToolDisplay block"
    );
    let tool_block = state
        .transcript
        .blocks()
        .first()
        .ok_or("tool block should exist")?;
    assert!(
        matches!(tool_block.source, BlockSource::Tool { preview: None, .. }),
        "the Tool block must keep preview None"
    );
    let preview_block = state
        .transcript
        .blocks()
        .get(1)
        .ok_or("preview block should exist")?;
    assert!(
        matches!(preview_block.source, BlockSource::ToolDisplay { .. }),
        "the preview must be a ToolDisplay block"
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
    dispatch_ui_event(
        &mut state,
        UiEvent::Warning {
            message: "test warning".to_string(),
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
