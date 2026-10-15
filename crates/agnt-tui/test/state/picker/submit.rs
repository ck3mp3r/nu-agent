use super::*;

/// Submitting a picker with no selection closes it without queueing a switch
/// request; the command palette with zero matches opens no panel; a selection
/// resolves to its payload switch action.
#[test]
fn picker_submit_resolution() -> Result<()> {
    // -- Exec & Check: no selection queues no switch request.
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

    // -- Setup & Fixtures: the command palette with zero matches.
    let mut palette = AppState::default();
    open_command_palette_for_test(&mut palette);
    type_query(&mut palette, "zzz")?;

    // -- Exec
    let changed =
        dispatch_terminal_event(&mut palette, &TerminalEvent::Key(TerminalKey::Enter), None);

    // -- Check: the palette closes and no panel or launch request appears.
    assert!(changed, "Enter should be consumed by the palette");
    assert_eq!(palette.picker.render_kind(), None, "palette should close");
    assert_eq!(palette.info_panel, None, "Help panel must not open");
    assert_eq!(
        palette.take_next_launch_request(),
        None,
        "no launch request may be queued"
    );

    // -- Setup & Fixtures: a model picker with a selection.
    let mut selected = AppState::default();
    selected.set_picker_options(ActivePicker::Model, test_model_options());
    selected.picker.open(ActivePicker::Model);

    // -- Exec
    let changed =
        dispatch_terminal_event(&mut selected, &TerminalEvent::Key(TerminalKey::Enter), None);

    // -- Check: the selection resolves to its payload identity.
    assert!(changed, "submit should be handled");
    assert_eq!(
        selected.take_next_switch_request(),
        Some(SwitchRequest::Model(
            "anthropic/claude-3-5-sonnet".to_string()
        ))
    );
    assert_eq!(
        selected.picker.render_kind(),
        None,
        "picker closes after submit"
    );
    Ok(())
}
