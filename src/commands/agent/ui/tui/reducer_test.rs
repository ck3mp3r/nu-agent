use crate::commands::agent::ui::{
    event::UiEvent,
    tui::{
        reducer::{ESC_ABORT_CONFIRM_STATUS, ReducerInput, UserAction, reduce_with_cancel_controller},
        state::{AppState, InputMode, TranscriptRole, UiPhase},
    },
};

fn assert_reducer_invariants(state: &AppState) {
    assert_eq!(state.input.locked, state.phase != UiPhase::Idle);
    assert_eq!(state.abort.pending, state.phase == UiPhase::AbortPending);
    assert!(state.input.cursor <= state.input.buffer.len());
    assert!(state.input.buffer.is_char_boundary(state.input.cursor));
    if state.phase == UiPhase::Idle {
        assert!(!state.is_active_cycle());
    }
}

fn busy_state_with_clean_transcript() -> AppState {
    let mut state = AppState::new();
    for ch in "run".chars() {
        reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::InsertChar(ch)), None);
    }
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);
    state.transcript_preview.clear();
    state
}

#[test]
fn submit_transition_is_deterministic_and_locks_input() {
    let mut state = AppState::new();
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::InsertChar('s')),
        None,
    );
    for ch in "tatus pods".chars() {
        reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::InsertChar(ch)), None);
    }

    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);

    assert_eq!(state.phase, UiPhase::Busy);
    assert!(state.input.locked);
    assert!(state.input.buffer.is_empty());
    assert_eq!(state.transcript_preview.len(), 1);
    assert_eq!(state.transcript_preview[0].role, TranscriptRole::User);
    assert_eq!(state.transcript_preview[0].text, "status pods");
}

#[test]
fn table_driven_ui_event_mapping_keeps_completed_as_finalize_boundary() {
    let mut state = AppState::new();
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::InsertChar('p')),
        None,
    );
    for ch in "rompt".chars() {
        reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::InsertChar(ch)), None);
    }
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);

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
        reduce_with_cancel_controller(&mut state, ReducerInput::Event(event), None);
        assert_eq!(state.phase, UiPhase::Busy);
        assert!(state.input.locked);
    }

    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::Event(UiEvent::Completed { tool_calls: 1 }),
        None,
    );

    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.input.locked);
    assert!(!state.abort.pending);
}

#[test]
fn esc_then_esc_confirm_moves_into_abort_requested_without_unlocking() {
    let mut state = AppState::new();
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::InsertChar('d')),
        None,
    );
    for ch in "o work".chars() {
        reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::InsertChar(ch)), None);
    }
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);

    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Esc), None);
    assert_eq!(state.phase, UiPhase::AbortPending);
    assert!(state.abort.pending);
    assert_eq!(state.status_line, ESC_ABORT_CONFIRM_STATUS);

    let before_markers = state.transcript_preview.len();
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::EscConfirm), None);
    assert_eq!(state.phase, UiPhase::AbortPending);
    assert!(state.abort.pending);
    assert_eq!(state.status_line, "Abort requested.");
    assert_eq!(state.transcript_preview.len(), before_markers + 1);
    assert_eq!(
        state.transcript_preview.last().map(|line| line.role),
        Some(TranscriptRole::System)
    );
    assert_eq!(
        state.transcript_preview.last().map(|line| line.text.as_str()),
        Some("[abort requested]")
    );
}

#[test]
fn completed_event_clears_pending_and_unlocks_input() {
    let mut state = AppState::new();
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::InsertChar('d')),
        None,
    );
    for ch in "o work".chars() {
        reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::InsertChar(ch)), None);
    }
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Esc), None);

    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::Event(UiEvent::Completed { tool_calls: 0 }),
        None,
    );

    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.abort.pending);
    assert!(!state.input.locked);
    assert!(state.status_line.is_empty());
}

#[test]
fn locked_input_prevents_typing_and_submission() {
    let mut state = AppState::new();
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::InsertChar('f')),
        None,
    );
    for ch in "irst".chars() {
        reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::InsertChar(ch)), None);
    }
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);

    for ch in "second".chars() {
        reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::InsertChar(ch)), None);
    }
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);

    assert!(state.input.buffer.is_empty());
    assert_eq!(state.transcript_preview.len(), 1);
    assert_eq!(state.transcript_preview[0].role, TranscriptRole::User);
    assert_eq!(state.transcript_preview[0].text, "first");
}

#[test]
fn submit_whitespace_only_prompt_is_noop() {
    let mut state = AppState::new();
    for ch in [' ', ' ', '\t', '\n', ' '] {
        reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::InsertChar(ch)), None);
    }

    let before = state.clone();
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);

    assert_eq!(state, before);
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.input.locked);
    assert!(state.transcript_preview.is_empty());
}

#[test]
fn race_completion_before_second_escape_prevents_reentry_into_abort_pending() {
    let mut state = AppState::new();
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::InsertChar('r')),
        None,
    );
    for ch in "ace".chars() {
        reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::InsertChar(ch)), None);
    }
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);

    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Esc), None);
    assert_eq!(state.phase, UiPhase::AbortPending);

    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::Event(UiEvent::Completed { tool_calls: 0 }),
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
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::InsertChar('f')),
        None,
    );
    for ch in "inalize".chars() {
        reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::InsertChar(ch)), None);
    }
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Esc), None);

    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::Event(UiEvent::Completed { tool_calls: 0 }),
        None,
    );

    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.input.locked);
    assert!(!state.abort.pending);
    assert!(state.status_line.is_empty());
}

#[test]
fn reducer_supports_baseline_input_editing_with_cursor_controls() {
    let mut state = AppState::new();
    for ch in ['h', 'l', 'o'] {
        reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::InsertChar(ch)), None);
    }

    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::MoveCursorLeft),
        None,
    );
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::InsertChar('l')),
        None,
    );

    assert_eq!(state.input.buffer, "hllo");

    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::MoveCursorHome),
        None,
    );
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::Delete),
        None,
    );
    assert_eq!(state.input.buffer, "llo");
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
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::InsertChar('j')), None);
    assert_eq!(state.input.buffer, "j");

    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::EnterNormalModeFromChord),
        None,
    );

    assert!(state.input.buffer.is_empty());
    assert_eq!(state.input_mode, InputMode::Normal);
}

#[test]
fn line_scroll_actions_adjust_scroll_counters() {
    let mut state = AppState::new();
    state.set_transcript_viewport_lines(3);
    for i in 0..10 {
        state.push_transcript_line(TranscriptRole::Assistant, format!("line {i}"));
    }
    state.scroll_transcript_page_up(2);
    let before_follow_tail = state.transcript_follow_tail;

    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::ScrollLineDown),
        None,
    );
    assert_eq!(before_follow_tail, state.transcript_follow_tail);
    assert!(!state.transcript_follow_tail);

    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::ScrollLineDown),
        None,
    );
    assert_eq!(state.transcript_scroll_lines_from_bottom, 0);
    assert!(state.transcript_follow_tail);
}

#[test]
fn focus_and_jump_actions_mutate_focus_and_scroll_targets() {
    let mut state = AppState::new();
    state.enter_normal_mode();
    state.set_transcript_viewport_lines(3);
    for i in 0..10 {
        state.push_transcript_line(TranscriptRole::Assistant, format!("line {i}"));
    }
    state.transcript_follow_tail = false;
    state.transcript_scroll_lines_from_bottom = 9;

    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::FocusPaneRight),
        None,
    );
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::FocusPaneLeft),
        None,
    );

    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::ScrollToBottom),
        None,
    );
    assert!(state.transcript_follow_tail);
    assert_eq!(state.transcript_scroll_lines_from_bottom, 0);

    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::ScrollToTop),
        None,
    );
    assert!(!state.transcript_follow_tail);
    assert_eq!(state.transcript_scroll_lines_from_bottom, 7);
}

#[test]
fn assistant_message_is_appended_to_transcript_before_completed_unlock() {
    let mut state = AppState::new();
    for ch in "ping".chars() {
        reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::InsertChar(ch)), None);
    }
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);
    assert!(state.input.locked);

    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::Event(UiEvent::AssistantMessage {
            text: "pong".to_string(),
        }),
        None,
    );

    assert!(state.input.locked);
    assert_eq!(
        state
            .transcript_preview
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        vec!["ping", "────────────────", "pong"]
    );
    assert_eq!(state.transcript_preview[0].role, TranscriptRole::User);
    assert_eq!(state.transcript_preview[1].role, TranscriptRole::Separator);
    assert_eq!(state.transcript_preview[2].role, TranscriptRole::Assistant);

    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::Event(UiEvent::Completed { tool_calls: 0 }),
        None,
    );

    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.input.locked);
}

#[test]
fn scroll_events_pause_and_resume_transcript_follow_tail() {
    let mut state = AppState::new();
    state.set_transcript_viewport_lines(3);
    for i in 0..10 {
        state.push_transcript_line(TranscriptRole::Assistant, format!("line {i}"));
    }

    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::ScrollPageUp), None);
    assert!(!state.transcript_follow_tail);
    assert!(state.transcript_scroll_lines_from_bottom > 0);

    let scroll_before = state.transcript_scroll_lines_from_bottom;
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::ScrollPageDown),
        None,
    );
    assert!(state.transcript_scroll_lines_from_bottom < scroll_before);

    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::ScrollPageDown),
        None,
    );
    assert!(state.transcript_follow_tail);
    assert_eq!(state.transcript_scroll_lines_from_bottom, 0);
}

#[test]
fn tool_end_transcript_line_shows_args_summary_without_result_payload_dump() {
    let mut state = AppState::new();
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::Event(UiEvent::ToolEnd {
            name: "k8s__list_pods".to_string(),
            source: "mcp".to_string(),
            arguments: r#"{"namespace":"prod"}"#.to_string(),
            success: true,
            result: "[{\"name\":\"api-0\",\"ns\":\"prod\"}]".to_string(),
            error_kind: None,
            message: None,
        }),
        None,
    );

    assert_eq!(state.transcript_preview.len(), 1);
    let line = &state.transcript_preview[0];
    assert_eq!(line.role, TranscriptRole::Tool);
    assert!(line.text.starts_with("tool[k8s__list_pods] args="));
    assert!(line.text.contains("namespace"));
    assert!(!line.text.contains("api-0"));
    assert!(!line.text.contains("[{"));
}

#[test]
fn llm_end_event_updates_latest_and_rolling_token_usage() {
    let mut state = AppState::new();

    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::Event(UiEvent::LlmEnd {
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
        ReducerInput::Event(UiEvent::LlmEnd {
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
            name: "tool_end_appends_tool_line_and_thinking",
            event: UiEvent::ToolEnd {
                name: "k8s__list_pods".to_string(),
                source: "mcp".to_string(),
                arguments: r#"{"namespace":"prod"}"#.to_string(),
                success: true,
                result: "[]".to_string(),
                error_kind: None,
                message: None,
            },
            pre: busy_empty_status,
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
        let before = state.clone();
        reduce_with_cancel_controller(&mut state, ReducerInput::Event(case.event), None);

        match case.name {
            "llm_start_from_idle_moves_busy" => {
                assert_eq!(state.phase, UiPhase::Busy);
                assert!(state.input.locked);
            }
            "llm_start_when_busy_is_noop" => {
                assert_eq!(state, before);
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
            "tool_end_appends_tool_line_and_thinking" => {
                assert_eq!(state.transcript_preview.len(), 1);
                assert_eq!(state.transcript_preview[0].role, TranscriptRole::Tool);
                assert!(state.transcript_preview[0].text.starts_with("tool[k8s__list_pods] args="));
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
                assert_eq!(
                    state
                        .transcript_preview
                        .iter()
                        .map(|line| (line.role, line.text.as_str()))
                        .collect::<Vec<_>>(),
                    vec![
                        (TranscriptRole::Assistant, "line 1"),
                        (TranscriptRole::Assistant, "line 2"),
                    ]
                );
            }
            "completed_finalizes_cycle" => {
                assert_eq!(state.phase, UiPhase::Idle);
                assert!(!state.input.locked);
                assert!(!state.abort.pending);
                assert!(state.status_line.is_empty());
            }
            _ => unreachable!("unknown case: {}", case.name),
        }

        assert_reducer_invariants(&state);
    }
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
        for ch in "draft".chars() {
            reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::InsertChar(ch)), None);
        }
        state
    }

    fn busy() -> AppState {
        busy_state_with_clean_transcript()
    }

    fn busy_with_resize_applied() -> AppState {
        let mut state = busy_state_with_clean_transcript();
        state.set_transcript_viewport_lines(29);
        state
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
            name: "quit_busy_is_noop",
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
        let before = state.clone();
        reduce_with_cancel_controller(&mut state, ReducerInput::User(case.action), None);

        match case.name {
            "history_up_noop"
            | "history_down_noop"
            | "complete_forward_noop"
            | "complete_backward_noop"
            | "resize_noop"
            | "quit_idle_with_text_is_noop"
            | "quit_busy_is_noop" => {
                assert_eq!(state, before);
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
                assert_eq!(state.input.buffer, "q");
                assert_eq!(state.input.cursor, 1);
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
        ReducerInput::Event(UiEvent::AssistantMessage {
            text: " \n\t\n".to_string(),
        }),
        None,
    );

    assert!(state.transcript_preview.is_empty());
    assert_reducer_invariants(&state);
}
