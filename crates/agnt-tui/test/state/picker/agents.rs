use super::*;

/// Agent picker open/close resets state, launch and switch requests queue and
/// drain in order, and the active agent identity is recorded.
#[test]
fn agent_picker_open_close_and_requests() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Agent, test_agent_options());
    if let Some(s) = state.picker.active_state_mut() {
        s.query = "leftover".to_string();
        s.selection = 2;
    }

    // -- Exec: open resets query and selection.
    state.picker.open(ActivePicker::Agent);

    // -- Check
    assert_eq!(state.picker.render_kind(), Some(PickerRenderKind::Agent));
    let s = state.picker.active_state().ok_or("picker should be open")?;
    assert_eq!(s.query, "");
    assert_eq!(s.selection, 0);

    // -- Exec: close clears the active picker.
    if let Some(s) = state.picker.active_state_mut() {
        s.query = "al".to_string();
        s.selection = 1;
    }
    state.picker.close();

    // -- Check
    assert_eq!(state.picker.render_kind(), None);

    // -- Exec & Check: launch requests queue and drain in order.
    let mut launch = AppState::default();
    assert_eq!(launch.take_next_launch_request(), None);
    launch.queue_launch_request(SharedUiAction::Agents);
    assert_eq!(
        launch.take_next_launch_request(),
        Some(SharedUiAction::Agents)
    );
    assert_eq!(launch.take_next_launch_request(), None);

    // -- Exec & Check: switch requests queue and drain in order.
    let mut switching = AppState::default();
    switching.set_picker_options(ActivePicker::Agent, test_agent_options());
    switching.picker.open(ActivePicker::Agent);
    let opt = switching
        .picker
        .active_state()
        .ok_or("picker should be open")?
        .selected()
        .ok_or("should have selected option")?;
    switching.queue_switch_request(SwitchRequest::Agent(opt.id.clone()));
    switching
        .picker
        .active_state_mut()
        .ok_or("picker should be open")?
        .move_down();
    let opt = switching
        .picker
        .active_state()
        .ok_or("picker should be open")?
        .selected()
        .ok_or("should have selected option")?;
    switching.queue_switch_request(SwitchRequest::Agent(opt.id.clone()));
    assert_eq!(
        switching.take_next_switch_request(),
        Some(SwitchRequest::Agent("alpha".to_string()))
    );
    assert_eq!(
        switching.take_next_switch_request(),
        Some(SwitchRequest::Agent("beta".to_string()))
    );
    assert_eq!(switching.take_next_switch_request(), None);

    // -- Exec & Check: the active agent identity is recorded.
    let mut identity = AppState::default();
    identity.set_picker_options(ActivePicker::Agent, test_agent_options());
    identity.set_active_agent_identity("beta");
    assert_eq!(
        identity.status.identity.active_agent_identity(),
        Some("beta")
    );

    // -- Setup & Fixtures: filtering and selection.
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Agent, test_agent_options());
    state.picker.open(ActivePicker::Agent);

    // -- Exec & Check: an empty query returns all options.
    assert_eq!(
        state
            .picker
            .active_state()
            .ok_or("picker should be open")?
            .filtered()
            .len(),
        3
    );

    // -- Exec & Check: an uppercase query matches case-insensitively.
    type_query(&mut state, "ALPHA")?;
    let filtered = state
        .picker
        .active_state()
        .ok_or("picker should be open")?
        .filtered();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].id, "alpha");

    // -- Exec & Check: a non-prefix query matches the description.
    state
        .picker
        .active_state_mut()
        .ok_or("picker should be open")?
        .query
        .clear();
    type_query(&mut state, "Gamma agent")?;
    let filtered = state
        .picker
        .active_state()
        .ok_or("picker should be open")?
        .filtered();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].id, "gamma");

    // -- Exec & Check: a no-match query returns an empty list.
    state
        .picker
        .active_state_mut()
        .ok_or("picker should be open")?
        .query
        .clear();
    type_query(&mut state, "zzz")?;
    assert!(
        state
            .picker
            .active_state()
            .ok_or("picker should be open")?
            .filtered()
            .is_empty()
    );

    // -- Setup & Fixtures: a fresh picker.
    let mut selection = AppState::default();
    selection.set_picker_options(ActivePicker::Agent, test_agent_options());
    selection.picker.open(ActivePicker::Agent);

    // -- Exec & Check: the selected option follows the selection index.
    let first = selection
        .picker
        .active_state()
        .ok_or("picker should be open")?
        .selected()
        .ok_or("should have first selected option")?;
    assert_eq!(first.id, "alpha");
    selection
        .picker
        .active_state_mut()
        .ok_or("picker should be open")?
        .move_down();
    let second = selection
        .picker
        .active_state()
        .ok_or("picker should be open")?
        .selected()
        .ok_or("should have second selected option")?;
    assert_eq!(second.id, "beta");
    selection
        .picker
        .active_state_mut()
        .ok_or("picker should be open")?
        .move_down();
    let third = selection
        .picker
        .active_state()
        .ok_or("picker should be open")?
        .selected()
        .ok_or("should have third selected option")?;
    assert_eq!(third.id, "gamma");

    // -- Exec & Check: move up wraps from the first to the last.
    let mut wrapping = AppState::default();
    wrapping.set_picker_options(ActivePicker::Agent, test_agent_options());
    wrapping.picker.open(ActivePicker::Agent);
    assert_eq!(
        wrapping
            .picker
            .active_state()
            .ok_or("picker should be open")?
            .selection,
        0
    );
    wrapping
        .picker
        .active_state_mut()
        .ok_or("picker should be open")?
        .move_up();
    assert_eq!(
        wrapping
            .picker
            .active_state()
            .ok_or("picker should be open")?
            .selection,
        2
    );

    // -- Exec & Check: move down wraps from the last to the first.
    wrapping
        .picker
        .active_state_mut()
        .ok_or("picker should be open")?
        .move_down();
    assert_eq!(
        wrapping
            .picker
            .active_state()
            .ok_or("picker should be open")?
            .selection,
        0
    );
    Ok(())
}
