use super::*;
use crate::test_support::entry_text;

#[test]
fn submit_path_appends_prompt_and_keeps_input_editable() {
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("s".to_string()),
        ..Default::default()
    };

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);

    assert!(changed);
    assert_eq!(state.phase, UiPhase::Busy);
    assert!(!state.input_locked);
    let _ = state.take_next_prompt_for_execution();
    // [User] — no leading or trailing spacer under the unified spacer rule
    assert_eq!(state.transcript.len(), 1);
    assert_eq!(entry_text(&state.transcript.entries()[0]), "s");
}

#[test]
fn dispatch_edit_actions_are_noops() {
    // Backspace and cursor movement are handled by TextArea, not the dispatch path.
    let mut state = AppState::default();

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Backspace),
        None,
    );
    assert!(!changed);

    // Alt+Enter and Shift+Enter are handled by the coordinator (TextArea),
    // not the dispatch path. The dispatch path's InsertNewline is a no-op.
    let mut state = AppState::default();

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::AltEnter), None);
    assert!(!changed);
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(state.transcript.entries().is_empty());

    // Enter still submits via pending_submit_text
    state.input.pending_submit_text = Some("h".to_string());
    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
    assert!(changed);
    assert_eq!(state.phase, UiPhase::Busy);
    let _ = state.take_next_prompt_for_execution();
    // [User] — no leading or trailing spacer under the unified spacer rule
    assert_eq!(state.transcript.len(), 1);
    assert_eq!(entry_text(&state.transcript.entries()[0]).trim(), "h");
}
