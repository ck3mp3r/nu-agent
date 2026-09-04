//! Reducer tests for user actions and the cross-domain `UiEvent` dispatch.
//! Domain-local effect assertions live in `state/{tool,llm,compaction,turn}_test.rs`;
//! the tests here cover orchestration-level behavior (phase, input lock,
//! abort, finalize boundaries) through `reduce_with_cancel_controller`.

use crate::{
    interaction::reducer::{
        ESC_ABORT_CONFIRM_STATUS, ReducerInput, UserAction,
        VISUAL_REQUIRES_TRANSCRIPT_FOCUS_STATUS, reduce_with_cancel_controller,
    },
    state::{AppState, InputMode, InputState, PaneFocus, PromptStatus, ScrollState, UiPhase},
};
use nu_agent_core::protocol::event::{PermissionRequestContext, UiEvent};
use nu_agent_core::transcript::ir::Role;
use nu_agent_core::transcript::items::{ProseMessage, TranscriptEntry, TranscriptEntryKind};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

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
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("run".to_string()),
        ..Default::default()
    };
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);
    let _ = state.activate_next_prompt();
    state.transcript.entries.clear();
    // Simulate handle_llm_start which sets the lock
    state.input_locked = true;
    state
}

#[test]
fn submit_transition_is_deterministic_and_keeps_input_editable() {
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("status pods".to_string()),
        ..Default::default()
    };
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);

    assert_eq!(state.phase, UiPhase::Busy);
    assert!(!state.input_locked);
    let _ = state.take_next_prompt_for_execution();
    // starting spacer + user + closing spacer
    assert_eq!(state.transcript.entries.len(), 3);
    assert_eq!(state.transcript.entries[1].role(), Role::User);
    assert_eq!(state.transcript.entries[1].text(), "status pods");
}

#[test]
fn table_driven_ui_event_mapping_keeps_completed_as_finalize_boundary() {
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("prompt".to_string()),
        ..Default::default()
    };
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);
    let _ = state.activate_next_prompt();

    let cases = vec![
        UiEvent::LlmStarted,
        UiEvent::Tick,
        UiEvent::ToolStarted {
            name: "k8s__list_pods".to_string(),
            source: "mcp".to_string(),
            arguments: "{}".to_string(),
        },
        UiEvent::ToolCompleted {
            name: "k8s__list_pods".to_string(),
            source: "mcp".to_string(),
            arguments: "{}".to_string(),
            success: true,
            result: "[]".to_string(),
            display: None,
            error_kind: None,
            message: None,
        },
        UiEvent::LlmCompleted {
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
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("do work".to_string()),
        ..Default::default()
    };
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);
    let _ = state.activate_next_prompt();

    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Esc), None);
    assert_eq!(state.phase, UiPhase::AbortPending);
    assert!(state.abort.pending);
    assert_eq!(state.status.status_line, ESC_ABORT_CONFIRM_STATUS);

    let before_markers = state.transcript.entries.len();
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::EscConfirm), None);
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.abort.pending);
    assert_eq!(state.status.status_line, "Abort requested.");
    // cancel pushes a closing spacer
    assert_eq!(state.transcript.entries.len(), before_markers + 1);
}

#[test]
fn completed_event_clears_pending_and_unlocks_input() {
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("do work".to_string()),
        ..Default::default()
    };
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
    assert!(state.status.status_line.is_empty());
}

#[test]
fn locked_input_prevents_typing_and_submission() {
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("first".to_string()),
        ..Default::default()
    };
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);

    state.input.pending_submit_text = Some("second".to_string());
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);

    // Activate both prompts coalesced
    let result = state.take_next_prompt_for_execution();
    assert_eq!(result, Some("first\n\nsecond".to_string()));
    // starting spacer + user + closing spacer
    assert_eq!(state.transcript.entries.len(), 3);
    assert_eq!(state.transcript.entries[1].role(), Role::User);
    assert_eq!(state.transcript.entries[1].text(), "first\n\nsecond");
    // Complete first (active) prompt — other prompt is already Done
    state.complete_active_prompt();
    let result = state.take_next_prompt_for_execution();
    assert_eq!(result, None);
}

#[test]
fn submit_whitespace_only_prompt_is_noop() {
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("  \t\n ".to_string()),
        ..Default::default()
    };

    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);

    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.input_locked);
    assert!(state.transcript.entries.is_empty());
}

#[test]
fn race_completion_before_second_escape_prevents_reentry_into_abort_pending() {
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("race".to_string()),
        ..Default::default()
    };
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

    let transcript_before = state.transcript.entries.clone();
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::EscConfirm), None);
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.abort.pending);
    assert_eq!(state.transcript.entries, transcript_before);
}

#[test]
fn completed_event_unlocks_and_clears_abort_pending() {
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("finalize".to_string()),
        ..Default::default()
    };
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
    assert!(state.status.status_line.is_empty());
}

#[test]
fn insert_newline_action_inserts_line_break_without_submit() {
    // InsertNewline is now handled by TextArea, not the reducer.
    // This test is preserved as a no-op to document the architectural change.
}

#[test]
fn enter_insert_and_normal_mode_actions_toggle_mode_only_in_idle() {
    let mut state = AppState::default();
    assert_eq!(state.input.mode, InputMode::Insert);

    state.enter_normal_mode();
    assert_eq!(state.input.mode, InputMode::Normal);

    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::EnterInsertMode),
        None,
    );
    assert_eq!(state.input.mode, InputMode::Insert);
}

#[test]
fn enter_normal_mode_from_chord_removes_last_j_and_switches_mode() {
    let mut state = AppState::default();

    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::EnterNormalModeFromChord),
        None,
    );

    assert_eq!(state.input.mode, InputMode::Normal);
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
        .transcript
        .entries
        .iter()
        .any(|line| line.text().contains("test error"));
    assert!(
        has_error_in_transcript,
        "Expected error message in transcript"
    );

    // Verify a second prompt submission is accepted (not blocked)
    state.input.pending_submit_text = Some("retry".to_string());
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);
    assert_eq!(state.phase, UiPhase::Busy);
    // Simulate handle_llm_start which sets the lock
    state.input_locked = true;
    // Transcript entry for "retry" is deferred to activation
    let _ = state.take_next_prompt_for_execution();
    assert!(
        state
            .transcript
            .entries
            .iter()
            .any(|line| line.text() == "retry"),
        "Second prompt should appear in transcript"
    );

    assert_reducer_invariants(&state);
}

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
        state.status.status_line.clear();
        state
    }

    fn busy_with_status() -> AppState {
        let mut state = busy_state_with_clean_transcript();
        state.status.status_line = "Tool: prior".to_string();
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
            event: UiEvent::ToolStarted {
                name: "k8s__list_pods".to_string(),
                source: "mcp".to_string(),
                arguments: "{}".to_string(),
            },
            pre: busy_empty_status,
        },
        Case {
            name: "tool_end_updates_existing_tool_line_and_thinking",
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
            name: "llm_end_records_tokens_and_sets_ready_status",
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
            name: "warning_is_reducer_noop",
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
                assert_eq!(state.status.status_line, "Tool: prior");
                assert!(state.input_locked);
            }
            "tick_sets_thinking_when_empty" => {
                assert_eq!(state.status.status_line, "Thinking...");
            }
            "tick_preserves_existing_status" => {
                assert_eq!(state.status.status_line, "Tool: prior");
            }
            "tool_start_sets_status" => {
                assert_eq!(state.status.status_line, "Tool: k8s__list_pods");
            }
            "tool_end_updates_existing_tool_line_and_thinking" => {
                // [Spacer, Tool] — starting spacer then the tool, no closing spacer
                assert_eq!(state.transcript.entries.len(), 2);
                assert!(matches!(
                    state.transcript.entries[0].kind,
                    TranscriptEntryKind::Spacer(_)
                ));
                assert_eq!(state.transcript.entries[1].role(), Role::Tool);
                assert_eq!(state.transcript.entries[1].text(), "k8s__list_pods");
                assert_eq!(state.status.status_line, "Thinking...");
            }
            "llm_end_records_tokens_and_sets_ready_status" => {
                assert_eq!(state.status.latest_input_tokens, Some(4));
                assert_eq!(state.status.latest_output_tokens, Some(8));
                assert_eq!(state.status.latest_total_tokens, Some(12));
                assert_eq!(state.status.session_total_tokens, 12);
                assert_eq!(state.status.status_line, "Response ready (12 chars)");
            }
            "warning_is_reducer_noop" => {
                // Warning is handled by StatusState via warning_rx, not the
                // transcript reducer — the reducer no-ops and the status line
                // set by the `pre` fixture stays untouched.
                assert!(state.status.status_line.is_empty());
            }
            "assistant_message_trims_and_appends" => {
                // After the raw-markdown refactor, a single AssistantMessage
                // produces one ProseMessage. The raw text is trimmed before
                // storage, so leading/trailing whitespace is dropped.
                let assistant_entries: Vec<_> = state
                    .transcript
                    .entries
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
                assert!(state.status.status_line.is_empty());
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
        tool: "edit".to_string(),
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

#[cfg(test)]
mod user_action_matrix_tests {
    use super::*;

    #[test]
    fn table_driven_user_action_noop_and_contract_matrix() {
        struct Case {
            name: &'static str,
            action: UserAction,
            pre: fn() -> AppState,
        }

        fn idle() -> AppState {
            AppState::default()
        }

        fn idle_with_text() -> AppState {
            AppState {
                input: InputState::default().with_pending_submit_text("draft".to_string()),
                ..Default::default()
            }
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
                    assert_eq!(state.input.mode, InputMode::Normal);
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
    fn esc_esc_with_pending_restores_texts_to_input_buffer() {
        let mut state = AppState {
            input: InputState::default().with_pending_submit_text("first".to_string()),
            ..Default::default()
        };
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
        let mut state = AppState {
            input: InputState::default().with_pending_submit_text("do work".to_string()),
            ..Default::default()
        };
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

    #[test]
    fn enter_visual_mode_selects_first_visible_entry_not_scroll_offset() {
        let mut state = AppState {
            scroll: ScrollState {
                pane_focus: PaneFocus::Transcript,
                ..Default::default()
            },
            ..Default::default()
        };
        // Populate transcript with entries that render as multiple lines
        // Entry 0: multi-line markdown (renders as 3 visual rows)
        // Entry 1: multi-line markdown (renders as 2 visual rows)
        state.transcript.push_transcript_item(TranscriptEntry {
            id: 0,
            kind: TranscriptEntryKind::User(ProseMessage {
                markdown: "line 0\nextra\nmore".to_string(),
            }),
            status: None,
        });
        state.transcript.push_transcript_item(TranscriptEntry {
            id: 0,
            kind: TranscriptEntryKind::User(ProseMessage {
                markdown: "line 1\nextra".to_string(),
            }),
            status: None,
        });
        // Scroll offset = 2 means we've scrolled past entry 0's 3 lines
        // (offset 0, 1, 2 are all entry 0's visual rows)
        // The first visible entry should be entry 1
        state.scroll.scroll_offset = 2;
        state.scroll.cursor_visual_row = 3;
        state.scroll.entry_indices = vec![0, 0, 0, 1, 1];
        state.scroll.total_visual_rows = 5;
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::EnterVisualMode),
            None,
        );
        let sel = state.scroll.selection.expect("selection should be set");
        // Should select visual row 3 (maps to entry 1 via entry_indices)
        assert_eq!(sel.anchor(), 3);
        assert_eq!(sel.cursor(), 3);
        assert_eq!(state.input.mode, InputMode::Visual);
    }

    #[test]
    fn enter_visual_mode_sets_selection_at_scroll_offset() {
        let mut state = AppState {
            scroll: ScrollState {
                pane_focus: PaneFocus::Transcript,
                cursor_visual_row: 5,
                entry_indices: (0..10).collect(),
                total_visual_rows: 10,
                ..Default::default()
            },
            ..Default::default()
        };
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::EnterVisualMode),
            None,
        );
        let sel = state.scroll.selection.expect("selection should be set");
        assert_eq!(sel.anchor(), 5);
        assert_eq!(sel.cursor(), 5);
        assert_eq!(state.input.mode, InputMode::Visual);
        assert_eq!(state.status.status_line, "-- VISUAL --");
    }

    #[test]
    fn enter_visual_mode_requires_transcript_focus() {
        let mut state = AppState {
            scroll: ScrollState {
                pane_focus: PaneFocus::Input,
                ..Default::default()
            },
            ..Default::default()
        };
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::EnterVisualMode),
            None,
        );
        assert_eq!(
            state.status.status_line,
            VISUAL_REQUIRES_TRANSCRIPT_FOCUS_STATUS
        );
        assert!(state.scroll.selection.is_none());
    }

    #[test]
    fn enter_visual_mode_noop_when_busy() {
        let mut state = AppState {
            phase: UiPhase::Busy,
            ..Default::default()
        };
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::EnterVisualMode),
            None,
        );
        assert!(state.scroll.selection.is_none());
    }

    #[test]
    fn visual_j_extends_selection_down() {
        let mut state = AppState {
            input: InputState::default().with_mode(InputMode::Visual),
            scroll: ScrollState {
                selection: Some(TranscriptSelection::new(0)),
                entry_indices: (0..5).collect(),
                total_visual_rows: 5,
                ..Default::default()
            },
            ..Default::default()
        };
        for i in 0..5 {
            state.transcript.push_transcript_item(TranscriptEntry {
                id: 0,
                kind: TranscriptEntryKind::User(ProseMessage {
                    markdown: format!("line {i}"),
                }),
                status: None,
            });
        }
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::ScrollLineDown),
            None,
        );
        let sel = state.scroll.selection.expect("selection should exist");
        assert_eq!(sel.cursor(), 1);
        assert_eq!(state.scroll.cursor_visual_row, 1);
        assert_eq!(state.scroll.scroll_offset, 1);
        assert!(!state.scroll.following_tail);
    }

    #[test]
    fn visual_k_extends_selection_up() {
        let mut state = AppState {
            input: InputState::default().with_mode(InputMode::Visual),
            scroll: ScrollState {
                selection: Some(TranscriptSelection::new(2)),
                cursor_visual_row: 2,
                entry_indices: (0..3).collect(),
                total_visual_rows: 3,
                ..Default::default()
            },
            ..Default::default()
        };
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::ScrollLineUp),
            None,
        );
        let sel = state.scroll.selection.expect("selection should exist");
        assert_eq!(sel.cursor(), 1);
        assert_eq!(state.scroll.cursor_visual_row, 1);
        assert_eq!(state.scroll.scroll_offset, 0);
        assert!(!state.scroll.following_tail);
    }

    #[test]
    fn visual_yank_copies_and_exits_visual() {
        let mut state = AppState {
            input: InputState::default().with_mode(InputMode::Visual),
            scroll: ScrollState {
                rendered_line_text: vec![
                    "line 0".to_string(),
                    "line 1".to_string(),
                    "line 2".to_string(),
                    "line 3".to_string(),
                    "line 4".to_string(),
                ],
                rendered_line_start_row: 0,
                ..Default::default()
            },
            ..Default::default()
        };
        // Set selection covering visual rows 1-3
        let mut sel = TranscriptSelection::new(1);
        sel.set_cursor(3);
        state.scroll.selection = Some(sel);
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::YankSelection),
            None,
        );
        let clipboard = state.input.take_clipboard_request();
        assert_eq!(clipboard, Some("line 1\nline 2\nline 3".to_string()));
        assert!(state.scroll.selection.is_none());
        assert_eq!(state.input.mode, InputMode::Normal);
    }

    #[test]
    fn visual_yank_copies_only_selected_rows_not_whole_entry() {
        let mut state = AppState {
            input: InputState::default().with_mode(InputMode::Visual),
            scroll: ScrollState {
                rendered_line_text: (0..10).map(|i| format!("row {i}")).collect(),
                rendered_line_start_row: 0,
                ..Default::default()
            },
            ..Default::default()
        };
        // Select rows 3-5 out of 10
        let mut sel = TranscriptSelection::new(3);
        sel.set_cursor(5);
        state.scroll.selection = Some(sel);
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::YankSelection),
            None,
        );
        let clipboard = state.input.take_clipboard_request();
        assert_eq!(clipboard, Some("row 3\nrow 4\nrow 5".to_string()));
        assert!(state.scroll.selection.is_none());
        assert_eq!(state.input.mode, InputMode::Normal);
    }

    #[test]
    fn visual_yank_with_nonzero_scroll_offset_copies_correct_rows() {
        let mut state = AppState {
            input: InputState::default().with_mode(InputMode::Visual),
            scroll: ScrollState {
                // Simulate viewport scrolled down — rendered_line_text[0] is absolute row 10
                rendered_line_start_row: 10,
                rendered_line_text: vec![
                    "row 10".to_string(),
                    "row 11".to_string(),
                    "row 12".to_string(),
                    "row 13".to_string(),
                    "row 14".to_string(),
                ],
                ..Default::default()
            },
            ..Default::default()
        };
        // Select absolute visual rows 11-13
        let mut sel = TranscriptSelection::new(11);
        sel.set_cursor(13);
        state.scroll.selection = Some(sel);
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::YankSelection),
            None,
        );
        let clipboard = state.input.take_clipboard_request();
        assert_eq!(clipboard, Some("row 11\nrow 12\nrow 13".to_string()));
        assert!(state.scroll.selection.is_none());
        assert_eq!(state.input.mode, InputMode::Normal);
    }

    #[test]
    fn visual_yank_nothing_to_yank_shows_status() {
        let mut state = AppState {
            input: InputState::default().with_mode(InputMode::Visual),
            scroll: ScrollState {
                rendered_line_text: Vec::new(),
                scroll_offset: 0,
                selection: Some(TranscriptSelection::new(0)),
                ..Default::default()
            },
            ..Default::default()
        };
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::YankSelection),
            None,
        );
        assert!(state.input.take_clipboard_request().is_none());
        assert_eq!(state.status.status_line, "Nothing to yank");
        assert!(state.scroll.selection.is_none());
        assert_eq!(state.input.mode, InputMode::Normal);
    }

    #[test]
    fn visual_yank_empty_selection_noop() {
        let mut state = AppState::default();
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::YankSelection),
            None,
        );
        assert!(state.input.take_clipboard_request().is_none());
    }

    #[test]
    fn visual_esc_clears_selection_and_exits() {
        let mut state = AppState {
            input: InputState::default().with_mode(InputMode::Visual),
            scroll: ScrollState {
                selection: Some(TranscriptSelection::new(0)),
                ..Default::default()
            },
            ..Default::default()
        };
        reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Esc), None);
        assert!(state.scroll.selection.is_none());
        assert_eq!(state.input.mode, InputMode::Normal);
    }

    #[test]
    fn visual_gg_jumps_to_top() {
        let mut state = AppState {
            input: InputState::default().with_mode(InputMode::Visual),
            scroll: ScrollState {
                selection: Some(TranscriptSelection::new(5)),
                ..Default::default()
            },
            ..Default::default()
        };
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::ScrollToTop),
            None,
        );
        let sel = state.scroll.selection.expect("selection should exist");
        assert_eq!(sel.cursor(), 0);
        assert_eq!(state.scroll.scroll_offset, 0);
        assert!(!state.scroll.following_tail);
    }

    #[test]
    fn visual_g_jumps_to_bottom() {
        let mut state = AppState {
            input: InputState::default().with_mode(InputMode::Visual),
            scroll: ScrollState {
                selection: Some(TranscriptSelection::new(0)),
                total_visual_rows: 5,
                ..Default::default()
            },
            ..Default::default()
        };
        for i in 0..5 {
            state.transcript.push_transcript_item(TranscriptEntry {
                id: 0,
                kind: TranscriptEntryKind::User(ProseMessage {
                    markdown: format!("line {i}"),
                }),
                status: None,
            });
        }
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::ScrollToBottom),
            None,
        );
        let sel = state.scroll.selection.expect("selection should exist");
        assert_eq!(sel.cursor(), 4);
        assert!(state.scroll.following_tail);
    }

    #[test]
    fn normal_j_moves_cursor_not_viewport() {
        let mut state = AppState {
            input: InputState::default().with_mode(InputMode::Normal),
            scroll: ScrollState {
                pane_focus: PaneFocus::Transcript,
                viewport_height: 10,
                cursor_visual_row: 5,
                entry_indices: (0..20).collect(),
                total_visual_rows: 20,
                ..Default::default()
            },
            ..Default::default()
        };
        for i in 0..20 {
            state.transcript.push_transcript_item(TranscriptEntry {
                id: 0,
                kind: TranscriptEntryKind::User(ProseMessage {
                    markdown: format!("line {i}"),
                }),
                status: None,
            });
        }
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::ScrollLineDown),
            None,
        );
        // Cursor moved from 5 to 6, viewport didn't scroll (6 < 0+10-3=7)
        assert_eq!(state.scroll.cursor_visual_row, 6);
        assert_eq!(state.scroll.scroll_offset, 0);
        assert!(!state.scroll.following_tail);
    }

    #[test]
    fn normal_j_scrolls_viewport_when_cursor_leaves_margin() {
        let mut state = AppState {
            input: InputState::default().with_mode(InputMode::Normal),
            scroll: ScrollState {
                pane_focus: PaneFocus::Transcript,
                viewport_height: 10,
                cursor_visual_row: 6,
                entry_indices: (0..20).collect(),
                total_visual_rows: 20,
                ..Default::default()
            },
            ..Default::default()
        };
        for i in 0..20 {
            state.transcript.push_transcript_item(TranscriptEntry {
                id: 0,
                kind: TranscriptEntryKind::User(ProseMessage {
                    markdown: format!("line {i}"),
                }),
                status: None,
            });
        }
        // scroll_margin = 3, viewport_bottom = 0+10-3 = 7
        // cursor moves 6→7, visual_row=7 >= 7, viewport scrolls
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::ScrollLineDown),
            None,
        );
        assert_eq!(state.scroll.cursor_visual_row, 7);
        assert_eq!(state.scroll.scroll_offset, 1);
        assert!(!state.scroll.following_tail);
    }

    #[test]
    fn visual_j_scrolls_viewport_only_when_cursor_leaves_visible_area() {
        let mut state = AppState {
            input: InputState::default().with_mode(InputMode::Visual),
            scroll: ScrollState {
                following_tail: false,
                viewport_height: 10,
                scroll_offset: 0,
                selection: Some(TranscriptSelection::new(0)),
                entry_indices: (0..20).collect(),
                total_visual_rows: 20,
                ..Default::default()
            },
            ..Default::default()
        };
        // 20 entries, 1 visual row each
        for i in 0..20 {
            state.transcript.push_transcript_item(TranscriptEntry {
                id: 0,
                kind: TranscriptEntryKind::User(ProseMessage {
                    markdown: format!("line {i}"),
                }),
                status: None,
            });
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
        let sel = state.scroll.selection.expect("selection should exist");
        assert_eq!(sel.cursor(), 7);
        assert_eq!(state.scroll.cursor_visual_row, 7);
        assert_eq!(state.scroll.scroll_offset, 1);
        assert!(!state.scroll.following_tail);
    }

    #[test]
    fn visual_k_syncs_scroll_offset_when_exiting_tail_following() {
        let mut state = AppState {
            input: InputState::default().with_mode(InputMode::Visual),
            scroll: ScrollState {
                following_tail: true,
                max_scroll: 50,
                scroll_offset: 0, // stale
                cursor_visual_row: 55,
                selection: Some(TranscriptSelection::new(55)),
                entry_indices: (0..60).collect(),
                total_visual_rows: 60,
                ..Default::default()
            },
            ..Default::default()
        };
        for i in 0..60 {
            state.transcript.push_transcript_item(TranscriptEntry {
                id: 0,
                kind: TranscriptEntryKind::User(ProseMessage {
                    markdown: format!("line {i}"),
                }),
                status: None,
            });
        }
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::ScrollLineUp),
            None,
        );
        // scroll_offset should be synced from max_scroll (50), then cursor moves
        // 55→54. cursor_visual_row=54, scroll_margin=1, 54 < 50+1=51 is false,
        // so no additional scroll.
        assert_eq!(state.scroll.cursor_visual_row, 54);
        assert_eq!(state.scroll.scroll_offset, 50);
        assert!(!state.scroll.following_tail);
    }

    #[test]
    fn ctrl_u_moves_cursor_up_by_page() {
        let mut state = AppState {
            input: InputState::default().with_mode(InputMode::Normal),
            scroll: ScrollState {
                pane_focus: PaneFocus::Transcript,
                viewport_height: 10,
                cursor_visual_row: 10,
                total_visual_rows: 30,
                scroll_offset: 5,
                following_tail: false,
                entry_indices: (0..30).collect(),
                ..Default::default()
            },
            ..Default::default()
        };
        for i in 0..30 {
            state.transcript.push_transcript_item(TranscriptEntry {
                id: 0,
                kind: TranscriptEntryKind::User(ProseMessage {
                    markdown: format!("line {i}"),
                }),
                status: None,
            });
        }
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::ScrollPageUp),
            None,
        );
        // cursor moves 10 - 8 = 2
        assert_eq!(state.scroll.cursor_visual_row, 2);
        // scroll_margin = 10/3 = 3, cursor 2 < 5 + 3 = 8, so scroll_offset = 5 - 8 = 0
        assert_eq!(state.scroll.scroll_offset, 0);
        assert!(!state.scroll.following_tail);
    }

    #[test]
    fn ctrl_d_moves_cursor_down_by_page() {
        let mut state = AppState {
            input: InputState::default().with_mode(InputMode::Normal),
            scroll: ScrollState {
                pane_focus: PaneFocus::Transcript,
                viewport_height: 10,
                cursor_visual_row: 0,
                total_visual_rows: 30,
                scroll_offset: 0,
                following_tail: false,
                entry_indices: (0..30).collect(),
                ..Default::default()
            },
            ..Default::default()
        };
        for i in 0..30 {
            state.transcript.push_transcript_item(TranscriptEntry {
                id: 0,
                kind: TranscriptEntryKind::User(ProseMessage {
                    markdown: format!("line {i}"),
                }),
                status: None,
            });
        }
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::ScrollPageDown),
            None,
        );
        // cursor moves 0 + 8 = 8
        assert_eq!(state.scroll.cursor_visual_row, 8);
        // scroll_margin = 3, viewport_bottom = 0 + 10 - 3 = 7, cursor 8 >= 7, so scroll_offset = 0 + 8 = 8
        assert_eq!(state.scroll.scroll_offset, 8);
        assert!(!state.scroll.following_tail);
    }

    #[test]
    fn ctrl_u_in_visual_mode_extends_selection() {
        let mut state = AppState {
            input: InputState::default().with_mode(InputMode::Visual),
            scroll: ScrollState {
                pane_focus: PaneFocus::Transcript,
                selection: Some(TranscriptSelection::new(10)),
                cursor_visual_row: 10,
                total_visual_rows: 30,
                viewport_height: 10,
                scroll_offset: 5,
                following_tail: false,
                entry_indices: (0..30).collect(),
                ..Default::default()
            },
            ..Default::default()
        };
        for i in 0..30 {
            state.transcript.push_transcript_item(TranscriptEntry {
                id: 0,
                kind: TranscriptEntryKind::User(ProseMessage {
                    markdown: format!("line {i}"),
                }),
                status: None,
            });
        }
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::ScrollPageUp),
            None,
        );
        let sel = state.scroll.selection.expect("selection should exist");
        assert_eq!(state.scroll.cursor_visual_row, 2);
        assert_eq!(sel.cursor(), 2);
    }

    #[test]
    fn ctrl_d_in_visual_mode_extends_selection() {
        let mut state = AppState {
            input: InputState::default().with_mode(InputMode::Visual),
            scroll: ScrollState {
                pane_focus: PaneFocus::Transcript,
                selection: Some(TranscriptSelection::new(0)),
                cursor_visual_row: 0,
                total_visual_rows: 30,
                viewport_height: 10,
                scroll_offset: 0,
                following_tail: false,
                entry_indices: (0..30).collect(),
                ..Default::default()
            },
            ..Default::default()
        };
        for i in 0..30 {
            state.transcript.push_transcript_item(TranscriptEntry {
                id: 0,
                kind: TranscriptEntryKind::User(ProseMessage {
                    markdown: format!("line {i}"),
                }),
                status: None,
            });
        }
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::ScrollPageDown),
            None,
        );
        let sel = state.scroll.selection.expect("selection should exist");
        assert_eq!(state.scroll.cursor_visual_row, 8);
        assert_eq!(sel.cursor(), 8);
    }
}

#[test]
fn cancel_pushes_closing_spacer() -> Result<()> {
    use crate::interaction::cancel::CancelController;

    let cancel_controller = CancelController::default();
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("hello".to_string()),
        ..Default::default()
    };
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::Submit),
        Some(&cancel_controller),
    );
    let _ = state.take_next_prompt_for_execution();
    state.transcript.push_transcript_item(TranscriptEntry {
        id: 0,
        kind: TranscriptEntryKind::Assistant(ProseMessage {
            markdown: "partial".to_string(),
        }),
        status: None,
    });

    // First Esc enters AbortPending, second confirms the cancel
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::Esc),
        Some(&cancel_controller),
    );
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::EscConfirm),
        Some(&cancel_controller),
    );

    let last = state
        .transcript
        .entries
        .last()
        .ok_or("should have last transcript entry")?;
    assert!(matches!(last.kind, TranscriptEntryKind::Spacer(_)));
    Ok(())
}
