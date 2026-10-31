use super::*;
use crate::test_support::entry_text;

#[test]
fn busy_escape_abort_table() -> Result<()> {
    // Esc in busy insert mode switches to normal and arms abort confirmation.
    let (mut state, cancel_controller) = busy_state_with_controller();
    assert_eq!(state.phase, UiPhase::Busy);
    assert_eq!(state.input.mode, InputMode::Insert);
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );
    assert!(changed);
    assert_eq!(state.input.mode, InputMode::Normal);
    assert_eq!(state.phase, UiPhase::AbortPending);
    assert!(state.abort.pending);
    assert_eq!(state.status.message.status_line(), ESC_ABORT_CONFIRM_STATUS);
    assert!(!cancel_controller.is_cancel_requested());

    // A second Esc from busy insert aborts.
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );
    assert!(changed);
    assert!(cancel_controller.is_cancel_requested());
    assert!(state.status.message.status_line().is_empty());

    // The first escape in busy normal sets abort pending with the exact status text.
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

    // The second escape in abort pending after busy normal toggles cancel requested.
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );
    assert!(changed);
    assert!(cancel_controller.is_cancel_requested());
    assert_eq!(state.input.mode, InputMode::Insert);
    assert!(state.status.message.status_line().is_empty());
    Ok(())
}

#[test]
fn test_interaction_abort_cancel_esc_with_card_arms_then_confirms() -> Result<()> {
    // -- Setup & Fixtures: a busy turn in normal mode with a permission card up.
    let (mut state, cancel_controller) = busy_state_with_controller();
    state.enter_normal_mode();
    open_permission_prompt(&mut state);
    assert_eq!(state.phase, UiPhase::Busy);
    assert_eq!(state.input.mode, InputMode::Normal);

    // -- Exec: the first Esc arms the abort confirmation instead of denying.
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );

    // -- Check: armed, no decision, card still up.
    assert!(changed);
    assert_eq!(state.phase, UiPhase::AbortPending);
    assert!(state.abort.pending);
    assert_eq!(state.status.message.status_line(), ESC_ABORT_CONFIRM_STATUS);
    assert!(!cancel_controller.is_cancel_requested());
    assert!(
        state.permission.has_prompt(),
        "the card stays up while the abort is armed"
    );
    assert!(
        state.permission.take_next_submission().is_none(),
        "arming the abort must not submit a permission decision"
    );

    // -- Exec: the second Esc confirms the cancel.
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );

    // -- Check: the confirm reduction ran.
    assert!(changed);
    assert!(cancel_controller.is_cancel_requested());
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.abort.pending);
    assert!(state.status.message.status_line().is_empty());
    assert!(
        !state.permission.has_prompt(),
        "confirming the cancel must close the card immediately"
    );
    assert!(
        state.permission.take_next_submission().is_none(),
        "confirming the cancel must not submit a permission decision"
    );
    Ok(())
}

#[test]
fn idle_escape_table() {
    // Escape in idle does not request cancellation.
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

    // Idle normal Esc returns false.
    let mut state = AppState {
        input: InputState::default().with_mode(InputMode::Normal),
        ..Default::default()
    };
    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Esc), None);
    assert!(!changed);
    assert!(!state.abort.pending);
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

    // Typing in insert mode is handled by the coordinator, not the dispatch path.
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('s')),
        Some(&cancel_controller),
    );

    assert!(!changed);
    assert_eq!(state.transcript.len(), 1);
    assert_eq!(entry_text(&state.transcript.entries()[0]), "f");
}
