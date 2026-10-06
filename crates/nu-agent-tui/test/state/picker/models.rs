use super::*;

#[test]
fn inline_model_picker_opens_with_available_models() {
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Model, test_model_options());
    state.picker.open(ActivePicker::Model);

    assert_eq!(state.picker.render_kind(), Some(PickerRenderKind::Model));
    let s = state.picker.active_state().unwrap();
    assert_eq!(s.selection, 0);
    assert_eq!(s.query, "");
    assert_eq!(s.filtered().len(), 2);
}

#[test]
fn inline_model_picker_filters_incrementally() {
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Model, test_model_options());
    state.picker.open(ActivePicker::Model);

    for ch in "openai".chars() {
        state
            .picker
            .active_state_mut()
            .unwrap()
            .append_query_char(ch);
    }

    let filtered = state.picker.active_state().unwrap().filtered();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].id, "openai/gpt-4o-mini");
}

#[test]
fn inline_model_picker_moves_selection_deterministically() {
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Model, test_model_options());
    state.picker.open(ActivePicker::Model);

    state.picker.active_state_mut().unwrap().move_down();
    assert_eq!(state.picker.active_state().unwrap().selection, 1);

    state.picker.active_state_mut().unwrap().move_down();
    assert_eq!(state.picker.active_state().unwrap().selection, 0);

    state.picker.active_state_mut().unwrap().move_up();
    assert_eq!(state.picker.active_state().unwrap().selection, 1);
}

#[test]
fn inline_model_picker_closes_on_escape() {
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Model, test_model_options());
    state.picker.open(ActivePicker::Model);

    state.picker.close();

    assert_eq!(state.picker.render_kind(), None);
}

#[test]
fn inline_model_picker_uses_cached_startup_plugin_config_catalog() {
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Model,
        vec![
            ModelPickerOption {
                provider: "z-provider".to_string(),
                model: "z-model".to_string(),
                identity: "z-provider/z-model".to_string(),
                display: "z-provider / z-model".to_string(),
                active: false,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
            },
            ModelPickerOption {
                provider: "a-provider".to_string(),
                model: "a-model".to_string(),
                identity: "a-provider/a-model".to_string(),
                display: "a-provider / a-model".to_string(),
                active: true,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
            },
        ],
    );
    state.picker.open(ActivePicker::Model);

    let ordered = state.picker.active_state().unwrap().filtered();
    assert_eq!(ordered[0].id, "a-provider/a-model");
    assert_eq!(ordered[1].id, "z-provider/z-model");
}

#[test]
fn model_picker_query_changes_results_with_hydrated_catalog() {
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Model, test_model_options());
    state.picker.open(ActivePicker::Model);

    let all = state.picker.active_state().unwrap().filtered();
    assert_eq!(all.len(), 2);

    for ch in "claude".chars() {
        state
            .picker
            .active_state_mut()
            .unwrap()
            .append_query_char(ch);
    }
    let narrowed = state.picker.active_state().unwrap().filtered();
    assert_eq!(narrowed.len(), 1);
    assert_eq!(narrowed[0].id, "anthropic/claude-3-5-sonnet");
}

#[test]
fn model_picker_empty_catalog_shows_deterministic_empty_state() {
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Model, Vec::<PickerOption>::new());
    state.picker.open(ActivePicker::Model);

    assert_eq!(state.picker.render_kind(), Some(PickerRenderKind::Model));
    assert!(state.picker.active_state().unwrap().filtered().is_empty());
}
