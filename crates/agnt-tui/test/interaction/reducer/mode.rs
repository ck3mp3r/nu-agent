use super::*;

#[test]
fn mode_transition_table() {
    // Enter insert and normal mode actions toggle mode only in idle.
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

    // Enter normal mode from chord switches mode.
    let mut state = AppState::default();

    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::EnterNormalModeFromChord),
        None,
    );

    assert_eq!(state.input.mode, InputMode::Normal);
}
