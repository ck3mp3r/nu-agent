use super::*;

#[test]
fn test_open_agent_picker() {
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Agent, test_agent_options());
    if let Some(s) = state.picker.active_state_mut() {
        s.query = "leftover".to_string();
        s.selection = 2;
    }

    state.picker.open(ActivePicker::Agent);

    assert_eq!(state.picker.render_kind(), Some(PickerRenderKind::Agent));
    let s = state.picker.active_state().unwrap();
    assert_eq!(s.query, "");
    assert_eq!(s.selection, 0);
}

#[test]
fn test_close_agent_picker() {
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Agent, test_agent_options());
    state.picker.open(ActivePicker::Agent);
    if let Some(s) = state.picker.active_state_mut() {
        s.query = "al".to_string();
        s.selection = 1;
    }

    state.picker.close();

    assert_eq!(state.picker.render_kind(), None);
}

#[test]
fn test_queue_agent_picker_launch_request() {
    let mut state = AppState::default();

    state.queue_launch_request(SharedUiAction::Agents);

    assert_eq!(
        state.take_next_launch_request(),
        Some(SharedUiAction::Agents)
    );
}

#[test]
fn test_take_next_agent_picker_launch_request() {
    let mut state = AppState::default();

    assert_eq!(state.take_next_launch_request(), None);

    state.queue_launch_request(SharedUiAction::Agents);
    assert_eq!(
        state.take_next_launch_request(),
        Some(SharedUiAction::Agents)
    );

    assert_eq!(state.take_next_launch_request(), None);
}

#[test]
fn test_filtered_agent_picker_options_empty_query() {
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Agent, test_agent_options());
    state.picker.open(ActivePicker::Agent);

    let filtered = state.picker.active_state().unwrap().filtered();
    assert_eq!(filtered.len(), 3);
}

#[test]
fn test_filtered_agent_picker_options_with_query() {
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Agent, test_agent_options());
    state.picker.open(ActivePicker::Agent);

    for ch in "ALPHA".chars() {
        state
            .picker
            .active_state_mut()
            .unwrap()
            .append_query_char(ch);
    }
    let filtered = state.picker.active_state().unwrap().filtered();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].id, "alpha");

    state.picker.active_state_mut().unwrap().query.clear();
    for ch in "Gamma agent".chars() {
        state
            .picker
            .active_state_mut()
            .unwrap()
            .append_query_char(ch);
    }
    let filtered = state.picker.active_state().unwrap().filtered();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].id, "gamma");
}

#[test]
fn test_filtered_agent_picker_options_no_match() {
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Agent, test_agent_options());
    state.picker.open(ActivePicker::Agent);

    for ch in "zzz".chars() {
        state
            .picker
            .active_state_mut()
            .unwrap()
            .append_query_char(ch);
    }
    let filtered = state.picker.active_state().unwrap().filtered();
    assert!(filtered.is_empty());
}

#[test]
fn test_selected_agent_picker_option() -> Result<()> {
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Agent, test_agent_options());
    state.picker.open(ActivePicker::Agent);

    let first = state
        .picker
        .active_state()
        .unwrap()
        .selected()
        .ok_or("should have first selected option")?;
    assert_eq!(first.id, "alpha");

    state.picker.active_state_mut().unwrap().move_down();
    let second = state
        .picker
        .active_state()
        .unwrap()
        .selected()
        .ok_or("should have second selected option")?;
    assert_eq!(second.id, "beta");

    state.picker.active_state_mut().unwrap().move_down();
    let third = state
        .picker
        .active_state()
        .unwrap()
        .selected()
        .ok_or("should have third selected option")?;
    assert_eq!(third.id, "gamma");
    Ok(())
}

#[test]
fn test_queue_selected_agent_switch_request() {
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Agent, test_agent_options());
    state.picker.open(ActivePicker::Agent);

    state.picker.active_state_mut().unwrap().move_down();
    let opt = state.picker.active_state().unwrap().selected().unwrap();
    state.queue_switch_request(SwitchRequest::Agent(opt.id.clone()));

    let request = state.take_next_switch_request();
    assert_eq!(request, Some(SwitchRequest::Agent("beta".to_string())));
}

#[test]
fn test_take_next_agent_switch_request() {
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Agent, test_agent_options());
    state.picker.open(ActivePicker::Agent);

    let opt = state.picker.active_state().unwrap().selected().unwrap();
    state.queue_switch_request(SwitchRequest::Agent(opt.id.clone()));
    state.picker.active_state_mut().unwrap().move_down();
    let opt = state.picker.active_state().unwrap().selected().unwrap();
    state.queue_switch_request(SwitchRequest::Agent(opt.id.clone()));

    assert_eq!(
        state.take_next_switch_request(),
        Some(SwitchRequest::Agent("alpha".to_string()))
    );
    assert_eq!(
        state.take_next_switch_request(),
        Some(SwitchRequest::Agent("beta".to_string()))
    );
    assert_eq!(state.take_next_switch_request(), None);
}

#[test]
fn test_set_active_agent_identity() {
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Agent, test_agent_options());

    state.set_active_agent_identity("beta");

    assert_eq!(state.status.identity.active_agent_identity(), Some("beta"));
}

#[test]
fn test_agent_picker_move_up_wraps() {
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Agent, test_agent_options());
    state.picker.open(ActivePicker::Agent);

    assert_eq!(state.picker.active_state().unwrap().selection, 0);

    state.picker.active_state_mut().unwrap().move_up();
    assert_eq!(state.picker.active_state().unwrap().selection, 2);
}

#[test]
fn test_agent_picker_move_down_wraps() {
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Agent, test_agent_options());
    state.picker.open(ActivePicker::Agent);

    state.picker.active_state_mut().unwrap().move_down();
    state.picker.active_state_mut().unwrap().move_down();
    assert_eq!(state.picker.active_state().unwrap().selection, 2);

    state.picker.active_state_mut().unwrap().move_down();
    assert_eq!(state.picker.active_state().unwrap().selection, 0);
}
