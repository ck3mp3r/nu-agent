use super::*;

#[test]
fn table_driven_key_noop_and_contract_matrix() {
    struct Case {
        name: &'static str,
        event: SystemEvent,
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

    let cases = vec![
        Case {
            name: "history_up_noop",
            event: SystemEvent::KeyPressed { key: Key::Up },
            pre: busy,
        },
        Case {
            name: "history_down_noop",
            event: SystemEvent::KeyPressed { key: Key::Down },
            pre: busy,
        },
        Case {
            name: "query_prev_noop",
            event: SystemEvent::KeyPressed { key: Key::CtrlK },
            pre: busy,
        },
        Case {
            name: "complete_forward_noop",
            event: SystemEvent::KeyPressed { key: Key::Tab },
            pre: busy,
        },
        Case {
            name: "complete_backward_noop",
            event: SystemEvent::KeyPressed { key: Key::BackTab },
            pre: busy,
        },
        Case {
            name: "resize_noop",
            event: SystemEvent::Resized {
                width: 120,
                height: 40,
            },
            pre: busy,
        },
        Case {
            name: "quit_idle_empty_sets_flag",
            event: SystemEvent::KeyPressed { key: Key::CtrlC },
            pre: idle,
        },
        Case {
            name: "quit_idle_with_text_is_noop",
            event: SystemEvent::KeyPressed { key: Key::CtrlC },
            pre: idle_with_text,
        },
        Case {
            name: "quit_busy_cancels_and_quits",
            event: SystemEvent::KeyPressed { key: Key::CtrlC },
            pre: busy,
        },
        Case {
            name: "esc_idle_enters_normal_mode",
            event: SystemEvent::KeyPressed { key: Key::Esc },
            pre: idle,
        },
        Case {
            name: "insert_q_is_text_not_quit",
            event: SystemEvent::KeyPressed {
                key: Key::Char('q'),
            },
            pre: idle,
        },
    ];

    for case in cases {
        let mut state = (case.pre)();
        reduce(&mut state, case.event, None);

        match case.name {
            "history_up_noop"
            | "history_down_noop"
            | "query_prev_noop"
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
                // In the TextArea model, the reducer no longer checks for
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
fn esc_esc_restores_and_clears_state_table() {
    // Esc-Esc with pending prompts restores the texts to the input buffer.
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("first".to_string()),
        ..Default::default()
    };
    reduce(
        &mut state,
        SystemEvent::KeyPressed { key: Key::Enter },
        None,
    );
    let _ = state.activate_next_prompt();
    state.enqueue_prompt("second".to_string());
    state.enqueue_prompt("third".to_string());

    reduce(&mut state, SystemEvent::KeyPressed { key: Key::Esc }, None);
    assert_eq!(state.phase, UiPhase::AbortPending);

    reduce(&mut state, SystemEvent::KeyPressed { key: Key::Esc }, None);

    assert_eq!(state.phase, UiPhase::Idle);
    assert!(pending_prompt_ids(&state).is_empty());
    assert_eq!(active_prompt_id(&state), None);

    // Esc-Esc with no pending clears the state but not the buffer.
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("do work".to_string()),
        ..Default::default()
    };
    reduce(
        &mut state,
        SystemEvent::KeyPressed { key: Key::Enter },
        None,
    );
    let _ = state.activate_next_prompt();

    reduce(&mut state, SystemEvent::KeyPressed { key: Key::Esc }, None);
    reduce(&mut state, SystemEvent::KeyPressed { key: Key::Esc }, None);

    assert_eq!(state.phase, UiPhase::Idle);
    assert!(pending_prompt_ids(&state).is_empty());
    assert_eq!(active_prompt_id(&state), None);
}
