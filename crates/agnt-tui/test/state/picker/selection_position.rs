use super::*;

/// `position_selection_on` moves the selection to the matching id and leaves it
/// unchanged on no match; the Models action positions on the active identity.
#[test]
fn position_selection_on_matching_id() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Model, test_model_options());
    state.picker.open(ActivePicker::Model);

    // -- Exec
    state.picker.position_selection_on("openai/gpt-4o-mini");

    // -- Check: the selection moves to the matching option.
    assert_eq!(
        state
            .picker
            .active_state()
            .ok_or("picker should be open")?
            .selection,
        1
    );

    // -- Exec & Check: no match leaves the selection unchanged.
    state
        .picker
        .active_state_mut()
        .ok_or("picker should be open")?
        .selection = 1;
    state.picker.position_selection_on("no/such-model");
    assert_eq!(
        state
            .picker
            .active_state()
            .ok_or("picker should be open")?
            .selection,
        1
    );

    // -- Setup & Fixtures: the Models action with a matching active identity.
    let mut matching = AppState::default();
    matching.status.identity.active_model_identity = "openai/gpt-4o-mini".to_string();
    matching.set_picker_options(ActivePicker::Model, test_model_options());

    // -- Exec
    matching.reduce_ui_state_event(
        agnt_core::orchestrator::UiStateEvent::ExecuteSharedUiAction(SharedUiAction::Models),
    );

    // -- Check: the selection lands on the active identity row.
    assert_eq!(
        matching
            .picker
            .active_state()
            .ok_or("picker should be open")?
            .selection,
        1
    );

    // -- Setup & Fixtures: the Models action with no matching identity.
    let mut no_match = AppState::default();
    no_match.status.identity.active_model_identity = "no/such-model".to_string();
    no_match.set_picker_options(ActivePicker::Model, test_model_options());

    // -- Exec
    no_match.reduce_ui_state_event(
        agnt_core::orchestrator::UiStateEvent::ExecuteSharedUiAction(SharedUiAction::Models),
    );

    // -- Check: the selection stays at 0.
    assert_eq!(
        no_match
            .picker
            .active_state()
            .ok_or("picker should be open")?
            .selection,
        0
    );
    Ok(())
}
