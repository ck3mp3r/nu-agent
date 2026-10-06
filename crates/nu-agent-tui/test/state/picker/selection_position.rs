use super::*;

#[test]
fn position_selection_on_matching_id_sets_selection() {
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Model, test_model_options());
    state.picker.open(ActivePicker::Model);

    state.picker.position_selection_on("openai/gpt-4o-mini");

    assert_eq!(
        state.picker.active_state().unwrap().selection,
        1,
        "selection must move to the option whose id matches"
    );
}

#[test]
fn position_selection_on_no_match_leaves_selection_unchanged() {
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Model, test_model_options());
    state.picker.open(ActivePicker::Model);
    state.picker.active_state_mut().unwrap().selection = 1;

    state.picker.position_selection_on("no/such-model");

    assert_eq!(
        state.picker.active_state().unwrap().selection,
        1,
        "no match must leave selection unchanged"
    );
}

#[test]
fn models_action_positions_selection_on_active_identity() {
    let mut state = AppState::default();
    state.status.identity.active_model_identity = "openai/gpt-4o-mini".to_string();
    state.set_picker_options(ActivePicker::Model, test_model_options());

    state.reduce_ui_state_event(
        nu_agent_core::orchestrator::UiStateEvent::ExecuteSharedUiAction(SharedUiAction::Models),
    );

    assert_eq!(
        state.picker.active_state().unwrap().selection,
        1,
        "Models open must position selection on the active identity row"
    );
}

#[test]
fn models_action_no_match_leaves_selection_at_zero() {
    let mut state = AppState::default();
    state.status.identity.active_model_identity = "no/such-model".to_string();
    state.set_picker_options(ActivePicker::Model, test_model_options());

    state.reduce_ui_state_event(
        nu_agent_core::orchestrator::UiStateEvent::ExecuteSharedUiAction(SharedUiAction::Models),
    );

    assert_eq!(
        state.picker.active_state().unwrap().selection,
        0,
        "no match must leave selection at 0"
    );
}
