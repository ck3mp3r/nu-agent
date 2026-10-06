use super::*;

#[test]
fn picker_submit_without_selection_queues_no_switch_request() -> Result<()> {
    // -- Exec & Check
    for kind in [
        ActivePicker::Model,
        ActivePicker::Agent,
        ActivePicker::Session,
    ] {
        // Empty catalog: picker opened before options arrive.
        let mut state = AppState::default();
        state.set_picker_options(kind, Vec::<PickerOption>::new());
        state.picker.open(kind);

        let changed =
            dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);

        assert!(changed, "{kind:?} submit should be consumed");
        assert_eq!(state.picker.render_kind(), None, "{kind:?} should close");
        assert_eq!(
            state.take_next_switch_request(),
            None,
            "{kind:?} must not queue a switch request"
        );

        // Hydrated catalog with a query that matches zero options.
        let mut state = AppState::default();
        match kind {
            ActivePicker::Model => state.set_picker_options(kind, test_model_options()),
            ActivePicker::Agent => state.set_picker_options(kind, test_agent_options()),
            _ => state.set_picker_options(kind, test_session_options()),
        }
        state.picker.open(kind);
        type_query(&mut state, "zzz")?;

        let changed =
            dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);

        assert!(changed, "{kind:?} submit should be consumed");
        assert_eq!(state.picker.render_kind(), None, "{kind:?} should close");
        assert_eq!(
            state.take_next_switch_request(),
            None,
            "{kind:?} must not queue a switch request"
        );
    }
    Ok(())
}

#[test]
fn command_palette_submit_with_zero_matches_opens_no_panel() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    open_command_palette_for_test(&mut state);
    type_query(&mut state, "zzz")?;

    // -- Exec
    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);

    // -- Check
    assert!(changed, "Enter should be consumed by the palette");
    assert_eq!(state.picker.render_kind(), None, "palette should close");
    assert_eq!(state.info_panel, None, "Help panel must not open");
    assert_eq!(
        state.take_next_launch_request(),
        None,
        "no launch request may be queued"
    );
    Ok(())
}

#[test]
fn picker_submit_with_selection_resolves_payload_switch_action() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Model, test_model_options());
    state.picker.open(ActivePicker::Model);

    // -- Exec
    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);

    // -- Check
    assert!(changed, "submit should be handled");
    assert_eq!(
        state.take_next_switch_request(),
        Some(SwitchRequest::Model(
            "anthropic/claude-3-5-sonnet".to_string()
        )),
        "selection must resolve to its payload identity, not the placeholder"
    );
    assert_eq!(
        state.picker.render_kind(),
        None,
        "picker closes after submit"
    );
    Ok(())
}
