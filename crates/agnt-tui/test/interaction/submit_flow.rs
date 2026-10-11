use super::*;

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
    assert_eq!(state.transcript.blocks()[0].source.plain_text(), "s");
}

#[test]
fn backspace_and_cursor_movement_edit_in_dispatch_path() {
    // Backspace and cursor movement are now handled by TextArea, not the dispatch path.
    // The dispatch path's Backspace/Delete/MoveCursor actions are no-ops.
    let mut state = AppState::default();

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Backspace),
        None,
    );
    assert!(!changed);
}

#[test]
fn insert_mode_alt_and_shift_enter_insert_newline_while_enter_submits() {
    // Alt+Enter and Shift+Enter are now handled by the coordinator (TextArea),
    // not the dispatch path. The dispatch path's InsertNewline is a no-op.
    let mut state = AppState::default();

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::AltEnter), None);
    assert!(!changed);
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(state.transcript.blocks().is_empty());

    // Enter still submits via pending_submit_text
    state.input.pending_submit_text = Some("h".to_string());
    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
    assert!(changed);
    assert_eq!(state.phase, UiPhase::Busy);
    let _ = state.take_next_prompt_for_execution();
    // [User] — no leading or trailing spacer under the unified spacer rule
    assert_eq!(state.transcript.len(), 1);
    assert_eq!(state.transcript.blocks()[0].source.plain_text().trim(), "h");
}
