use super::*;

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
    // [User] — no leading or trailing spacer under the unified spacer rule
    assert_eq!(state.transcript.len(), 1);
    assert_eq!(
        state.transcript.blocks()[0].source.plain_text(),
        "status pods"
    );
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
            call_line: CallLine::from_json_summary("{}"),
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
    assert_eq!(state.status.message.status_line(), ESC_ABORT_CONFIRM_STATUS);

    let before_markers = state.transcript.len();
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::EscConfirm), None);
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.abort.pending);
    assert_eq!(state.input.mode, InputMode::Insert);
    assert_eq!(state.scroll.pane_focus, PaneFocus::Input);
    assert!(state.status.message.status_line().is_empty());
    // cancel no longer pushes a spacer — the unified rule separates blocks
    assert_eq!(state.transcript.len(), before_markers);
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
    assert!(state.status.message.status_line().is_empty());
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
    // [User] — no leading or trailing spacer under the unified spacer rule
    assert_eq!(state.transcript.len(), 1);
    assert_eq!(
        state.transcript.blocks()[0].source.plain_text(),
        "first\n\nsecond"
    );
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
    assert!(state.transcript.blocks().is_empty());
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

    let transcript_before = state.transcript.blocks().to_vec();
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::EscConfirm), None);
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.abort.pending);
    assert_eq!(state.transcript.blocks(), transcript_before);
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
    assert!(state.status.message.status_line().is_empty());
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
        .blocks()
        .iter()
        .any(|line| line.source.plain_text().contains("test error"));
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
            .blocks()
            .iter()
            .any(|line| line.source.plain_text() == "retry"),
        "Second prompt should appear in transcript"
    );

    assert_reducer_invariants(&state);
}
