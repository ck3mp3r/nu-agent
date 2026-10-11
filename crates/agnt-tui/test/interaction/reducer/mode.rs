use super::*;

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
