//! The idle and non-busy escape paths: no arm, no cancel, and typing stays
//! available while a prompt is active.

use super::*;
use crate::test_support::entry_text;

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

/// WHEN no abort is outstanding and the turn is idle, Esc behavior is unchanged.
#[test]
fn test_interaction_abort_cancel_idle_esc_unchanged() -> Result<()> {
    // -- Setup & Fixtures: idle insert mode, nothing armed.
    let mut state = AppState::default();
    let cancel_controller = CancelController::default();
    assert_eq!(state.input.mode, InputMode::Insert);

    // -- Exec
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );

    // -- Check: the plain mode exit, no notice, no arm, no cancel.
    assert!(changed);
    assert_eq!(state.input.mode, InputMode::Normal);
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.abort.pending);
    assert!(
        state.status.message.status_line().is_empty(),
        "an idle Esc has nothing to report"
    );
    assert!(!cancel_controller.is_cancel_requested());
    Ok(())
}
