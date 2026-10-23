use super::*;

/// Opening the session picker queues a refresh request that drains as a
/// `RefreshSessionPicker` orchestrator event.
#[test]
fn test_picker_session_refresh_emits_on_open() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    let cancel_controller = CancelController::default();

    // -- Exec
    state.reduce_ui_state_event(UiStateEvent::ExecuteSharedUiAction(
        SharedUiAction::Sessions,
    ));
    let events = state.take_pending_events(&cancel_controller);

    // -- Check
    assert_eq!(refresh_event_count(&events), 1);
    Ok(())
}

/// Two opens before a single drain still produce exactly one refresh event.
#[test]
fn test_picker_session_refresh_dedups_before_drain() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    let cancel_controller = CancelController::default();

    // -- Exec
    state.reduce_ui_state_event(UiStateEvent::ExecuteSharedUiAction(
        SharedUiAction::Sessions,
    ));
    state.reduce_ui_state_event(UiStateEvent::ExecuteSharedUiAction(
        SharedUiAction::Sessions,
    ));
    let events = state.take_pending_events(&cancel_controller);

    // -- Check
    assert_eq!(refresh_event_count(&events), 1);

    // -- Exec & Check: the flag is cleared by the drain.
    let drained_again = state.take_pending_events(&cancel_controller);
    assert_eq!(refresh_event_count(&drained_again), 0);
    Ok(())
}

/// Opening a different picker does not queue a session refresh.
#[test]
fn test_picker_session_refresh_ignores_other_actions() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    let cancel_controller = CancelController::default();

    // -- Exec
    state.reduce_ui_state_event(UiStateEvent::ExecuteSharedUiAction(SharedUiAction::Models));
    let events = state.take_pending_events(&cancel_controller);

    // -- Check
    assert_eq!(refresh_event_count(&events), 0);
    Ok(())
}

// region:    --- Test Support

fn refresh_event_count(events: &[OrchestratorEvent]) -> usize {
    events
        .iter()
        .filter(|event| {
            matches!(
                event,
                OrchestratorEvent::UiRequest(UiRequest::RefreshSessionPicker)
            )
        })
        .count()
}

// endregion: --- Test Support
