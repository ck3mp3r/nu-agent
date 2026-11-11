use super::*;

/// Opening the session picker queues the refresh hand the orchestrator answers.
#[test]
fn test_picker_session_refresh_emits_on_open() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec
    open_shared_ui_action(&mut state, SharedUiAction::Sessions);

    // -- Check: the refresh hand is waiting.
    assert!(state.take_next_session_picker_refresh_request().is_some());
    Ok(())
}

/// Two opens before a single drain still produce exactly one refresh hand.
#[test]
fn test_picker_session_refresh_dedups_before_drain() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec
    open_shared_ui_action(&mut state, SharedUiAction::Sessions);
    open_shared_ui_action(&mut state, SharedUiAction::Sessions);

    // -- Check: one hand, and the flag is cleared by the drain.
    assert!(state.take_next_session_picker_refresh_request().is_some());
    assert!(state.take_next_session_picker_refresh_request().is_none());
    Ok(())
}

/// Opening a different picker does not queue a session refresh.
#[test]
fn test_picker_session_refresh_ignores_other_actions() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec
    open_shared_ui_action(&mut state, SharedUiAction::Models);

    // -- Check
    assert!(state.take_next_session_picker_refresh_request().is_none());
    Ok(())
}

// region:    --- Test Support

/// Fold one shared UI action through the control fold and apply the launch it
/// queues, exactly as the live surface does.
fn open_shared_ui_action(state: &mut AppState, action: SharedUiAction) {
    fold_control(state, &ControlCommand::SharedUiAction { action });
    state.drain_launch_requests();
}

// endregion: --- Test Support
