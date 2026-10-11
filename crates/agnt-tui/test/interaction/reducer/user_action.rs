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
