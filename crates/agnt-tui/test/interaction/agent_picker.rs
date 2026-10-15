use super::*;

#[test]
fn agent_picker_lifecycle_table() -> Result<()> {
    // Open + insert char appends to the query.
    let mut state = setup_agent_picker_open();
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('a')),
        None,
    );
    assert!(changed);
    assert_eq!(state.picker.active_state().unwrap().query, "a");
    assert_eq!(state.picker.render_kind(), Some(PickerRenderKind::Agent));

    // Open + Esc closes the picker.
    let mut state = setup_agent_picker_open();
    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Esc), None);
    assert!(changed);
    assert_ne!(state.picker.render_kind(), Some(PickerRenderKind::Agent));

    // Open + submit queues a switch request and closes.
    let mut state = setup_agent_picker_open();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
    assert!(changed);
    assert_ne!(state.picker.render_kind(), Some(PickerRenderKind::Agent));
    assert_eq!(
        state.take_next_switch_request(),
        Some(SwitchRequest::Agent("beta".to_string()))
    );

    // Closed picker actions pass through normally.
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Agent,
        vec![agnt_core::protocol::picker::AgentPickerOption {
            name: "alpha".into(),
            description: None,
            display: "alpha".into(),
            builtin: false,
        }],
    );
    assert_ne!(state.picker.render_kind(), Some(PickerRenderKind::Agent));
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('x')),
        None,
    );
    assert!(!changed);
    assert_ne!(state.picker.render_kind(), Some(PickerRenderKind::Agent));

    // Ctrl-P moves the selection up.
    let mut state = setup_agent_picker_open();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    assert_eq!(state.picker.active_state().unwrap().selection, 1);
    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    assert!(changed);
    assert_eq!(state.picker.active_state().unwrap().selection, 0);
    assert_eq!(state.picker.render_kind(), Some(PickerRenderKind::Agent));
    Ok(())
}

#[test]
fn agent_picker_tab_table() {
    // Tab cycles the agent in insert mode.
    let mut state = AppState::default();
    assert_eq!(state.input.mode, InputMode::Insert);
    state.status.identity.agent_cycle_names = vec!["planner".to_string(), "maker".to_string()];
    state.set_active_agent_identity("planner");
    assert_ne!(state.picker.render_kind(), Some(PickerRenderKind::Agent));
    assert_ne!(state.picker.render_kind(), Some(PickerRenderKind::Model));
    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Tab), None);
    assert!(changed);
    assert_eq!(
        state.take_next_switch_request(),
        Some(SwitchRequest::Agent("maker".to_string()))
    );

    // Tab does not cycle in normal mode.
    let mut state = AppState::default();
    state.enter_normal_mode();
    state.status.identity.agent_cycle_names = vec!["planner".to_string(), "maker".to_string()];
    state.set_active_agent_identity("planner");
    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Tab), None);
    assert!(changed);
    assert_eq!(state.take_next_switch_request(), None);
    assert_eq!(state.scroll.pane_focus, crate::state::PaneFocus::Input);

    // Tab does not cycle when there are no builtins.
    let mut state = AppState::default();
    assert_eq!(state.input.mode, InputMode::Insert);
    state.status.identity.agent_cycle_names = Vec::new();
    let _changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Tab), None);
    assert_eq!(state.take_next_switch_request(), None);
}

#[test]
fn agent_slash_and_palette_share_same_action_handler() {
    // /agent slash command triggers the agent picker launch.
    let mut slash_state = AppState {
        input: InputState::default().with_pending_submit_text("/agent".to_string()),
        ..Default::default()
    };
    let _ = dispatch_terminal_event(
        &mut slash_state,
        &TerminalEvent::Key(TerminalKey::Enter),
        None,
    );
    assert_eq!(
        slash_state.take_next_launch_request(),
        Some(SharedUiAction::Agents)
    );
    assert_eq!(slash_state.take_next_prompt_for_execution(), None);

    // The command palette Agents action triggers the same path.
    let mut palette_state = AppState::default();
    let _ = dispatch_terminal_event(
        &mut palette_state,
        &TerminalEvent::Key(TerminalKey::CtrlP),
        None,
    );
    for _ in 0..5 {
        let _ = dispatch_terminal_event(
            &mut palette_state,
            &TerminalEvent::Key(TerminalKey::Down),
            None,
        );
    }
    let selected = palette_state
        .picker
        .active_state()
        .unwrap()
        .selected()
        .unwrap();
    assert_eq!(selected.id, "Agents");
    let _ = dispatch_terminal_event(
        &mut palette_state,
        &TerminalEvent::Key(TerminalKey::Enter),
        None,
    );
    assert_eq!(
        palette_state.take_next_launch_request(),
        Some(SharedUiAction::Agents)
    );
    assert_eq!(palette_state.take_next_prompt_for_execution(), None);
}
