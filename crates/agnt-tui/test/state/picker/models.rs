use super::*;

/// The model picker opens with the available models, filters incrementally,
/// moves selection deterministically, closes on escape, orders by provider
/// then identity, and shows a deterministic empty state for an empty catalog.
#[test]
fn model_picker_lifecycle() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Model, test_model_options());

    // -- Exec: open.
    state.picker.open(ActivePicker::Model);

    // -- Check: selection and query reset, all models listed.
    assert_eq!(state.picker.render_kind(), Some(PickerRenderKind::Model));
    let s = state.picker.active_state().ok_or("picker should be open")?;
    assert_eq!(s.selection, 0);
    assert_eq!(s.query, "");
    assert_eq!(s.filtered().len(), 2);

    // -- Exec & Check: filtering narrows the list.
    type_query(&mut state, "openai")?;
    let filtered = state
        .picker
        .active_state()
        .ok_or("picker should be open")?
        .filtered();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].id, "openai/gpt-4o-mini");

    // -- Setup & Fixtures: a fresh picker.
    let mut moving = AppState::default();
    moving.set_picker_options(ActivePicker::Model, test_model_options());
    moving.picker.open(ActivePicker::Model);

    // -- Exec & Check: selection moves deterministically and wraps.
    moving
        .picker
        .active_state_mut()
        .ok_or("picker should be open")?
        .move_down();
    assert_eq!(
        moving
            .picker
            .active_state()
            .ok_or("picker should be open")?
            .selection,
        1
    );
    moving
        .picker
        .active_state_mut()
        .ok_or("picker should be open")?
        .move_down();
    assert_eq!(
        moving
            .picker
            .active_state()
            .ok_or("picker should be open")?
            .selection,
        0
    );
    moving
        .picker
        .active_state_mut()
        .ok_or("picker should be open")?
        .move_up();
    assert_eq!(
        moving
            .picker
            .active_state()
            .ok_or("picker should be open")?
            .selection,
        1
    );

    // -- Exec & Check: close clears the picker.
    moving.picker.close();
    assert_eq!(moving.picker.render_kind(), None);

    // -- Setup & Fixtures: an unordered catalog.
    let mut ordered = AppState::default();
    ordered.set_picker_options(
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
    ordered.picker.open(ActivePicker::Model);

    // -- Check: ordered by provider then identity.
    let ordered_list = ordered
        .picker
        .active_state()
        .ok_or("picker should be open")?
        .filtered();
    assert_eq!(ordered_list[0].id, "a-provider/a-model");
    assert_eq!(ordered_list[1].id, "z-provider/z-model");

    // -- Setup & Fixtures: a hydrated catalog.
    let mut hydrated = AppState::default();
    hydrated.set_picker_options(ActivePicker::Model, test_model_options());
    hydrated.picker.open(ActivePicker::Model);

    // -- Exec & Check: a query changes the results.
    assert_eq!(
        hydrated
            .picker
            .active_state()
            .ok_or("picker should be open")?
            .filtered()
            .len(),
        2
    );
    type_query(&mut hydrated, "claude")?;
    let narrowed = hydrated
        .picker
        .active_state()
        .ok_or("picker should be open")?
        .filtered();
    assert_eq!(narrowed.len(), 1);
    assert_eq!(narrowed[0].id, "anthropic/claude-3-5-sonnet");

    // -- Setup & Fixtures: an empty catalog.
    let mut empty = AppState::default();
    empty.set_picker_options(ActivePicker::Model, Vec::<PickerOption>::new());
    empty.picker.open(ActivePicker::Model);

    // -- Check: a deterministic empty state.
    assert_eq!(empty.picker.render_kind(), Some(PickerRenderKind::Model));
    assert!(
        empty
            .picker
            .active_state()
            .ok_or("picker should be open")?
            .filtered()
            .is_empty()
    );
    Ok(())
}
