use super::*;

#[test]
fn first_escape_in_busy_normal_sets_abort_pending_with_exact_status_text() {
    let (mut state, cancel_controller) = busy_state_with_controller();
    state.enter_normal_mode();

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );

    assert!(changed);
    assert_eq!(state.phase, UiPhase::AbortPending);
    assert!(state.abort.pending);
    assert_eq!(state.input.mode, InputMode::Normal);
    assert_eq!(state.status.message.status_line(), ESC_ABORT_CONFIRM_STATUS);
    assert!(!cancel_controller.is_cancel_requested());
}

#[test]
fn second_escape_in_abort_pending_after_busy_normal_toggles_cancel_requested() {
    let (mut state, cancel_controller) = busy_state_with_controller();
    state.enter_normal_mode();
    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );

    assert!(changed);
    assert!(cancel_controller.is_cancel_requested());
    assert_eq!(state.input.mode, InputMode::Insert);
    assert!(state.status.message.status_line().is_empty());
}

#[test]
fn escape_in_idle_does_not_request_cancellation() {
    let mut state = AppState::default();
    let cancel_controller = CancelController::default();

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );

    assert!(changed);
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.abort.pending);
    assert!(!cancel_controller.is_cancel_requested());
    assert_eq!(state.input.mode, InputMode::Normal);
}

#[test]
fn typing_remains_available_while_prompt_is_active() {
    let mut state = AppState::default();
    let cancel_controller = CancelController::default();

    state.input.pending_submit_text = Some("f".to_string());
    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Enter),
        Some(&cancel_controller),
    );
    // Activate the prompt so the transcript entry is written
    let _ = state.take_next_prompt_for_execution();

    // Typing in insert mode is now handled by the coordinator, not the dispatch path.
    // The dispatch path's InsertChar is a no-op.
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('s')),
        Some(&cancel_controller),
    );

    assert!(!changed);
    // [User] — no leading or trailing spacer under the unified spacer rule
    assert_eq!(state.transcript.len(), 1);
    assert_eq!(state.transcript.blocks()[0].source.plain_text(), "f");
}

#[test]
fn esc_in_busy_insert_mode_switches_to_normal_and_arms_abort_confirmation() -> Result<()> {
    // -- Setup & Fixtures
    let (mut state, cancel_controller) = busy_state_with_controller();

    assert_eq!(state.phase, UiPhase::Busy);
    assert_eq!(state.input.mode, InputMode::Insert);

    // -- Exec
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );

    // -- Check
    assert!(changed);
    assert_eq!(state.input.mode, InputMode::Normal);
    assert_eq!(state.phase, UiPhase::AbortPending);
    assert!(state.abort.pending);
    assert_eq!(state.status.message.status_line(), ESC_ABORT_CONFIRM_STATUS);
    assert!(!cancel_controller.is_cancel_requested());
    Ok(())
}

#[test]
fn test_dispatch_terminal_event_second_esc_from_busy_insert_aborts() -> Result<()> {
    // -- Setup & Fixtures
    let (mut state, cancel_controller) = busy_state_with_controller();

    // -- Exec
    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );
    assert_eq!(state.status.message.status_line(), ESC_ABORT_CONFIRM_STATUS);
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );

    // -- Check
    assert!(changed);
    assert!(cancel_controller.is_cancel_requested());
    assert!(state.status.message.status_line().is_empty());
    Ok(())
}

#[test]
fn test_dispatch_terminal_event_idle_normal_esc_returns_false() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState {
        input: InputState::default().with_mode(InputMode::Normal),
        ..Default::default()
    };

    // -- Exec
    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Esc), None);

    // -- Check
    assert!(!changed);
    assert!(!state.abort.pending);
    Ok(())
}
