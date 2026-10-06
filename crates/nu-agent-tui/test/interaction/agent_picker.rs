use super::*;

#[test]
fn agent_picker_open_insert_char_appends_to_query() {
    let mut state = setup_agent_picker_open();

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('a')),
        None,
    );

    assert!(changed);
    assert_eq!(state.picker.active_state().unwrap().query, "a");
    assert_eq!(state.picker.render_kind(), Some(PickerRenderKind::Agent));
}

#[test]
fn agent_picker_open_esc_closes_picker() {
    let mut state = setup_agent_picker_open();

    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Esc), None);

    assert!(changed);
    assert_ne!(state.picker.render_kind(), Some(PickerRenderKind::Agent));
}

#[test]
fn agent_picker_open_submit_queues_switch_request_and_closes() {
    let mut state = setup_agent_picker_open();

    // Move to beta (sorted: alpha=0, beta=1, gamma=2)
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);

    assert!(changed);
    assert_ne!(state.picker.render_kind(), Some(PickerRenderKind::Agent));
    assert_eq!(
        state.take_next_switch_request(),
        Some(SwitchRequest::Agent("beta".to_string()))
    );
}

#[test]
fn agent_picker_closed_actions_pass_through_normally() {
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Agent,
        vec![nu_agent_core::protocol::picker::AgentPickerOption {
            name: "alpha".into(),
            description: None,
            display: "alpha".into(),
            builtin: false,
        }],
    );
    // Picker is NOT open
    assert_ne!(state.picker.render_kind(), Some(PickerRenderKind::Agent));

    // Char in dispatch path is now a no-op (handled by coordinator/TextArea)
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('x')),
        None,
    );
    assert!(!changed);
    assert_ne!(state.picker.render_kind(), Some(PickerRenderKind::Agent));
}

#[test]
fn agent_slash_and_palette_share_same_action_handler() {
    // /agent slash command triggers agent picker launch
    // InsertChar is now a no-op in the dispatch path (handled by TextArea).
    // Set pending_submit_text directly so Submit routes the slash command.
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

    // Command palette Agents action triggers same path
    let mut palette_state = AppState::default();
    let _ = dispatch_terminal_event(
        &mut palette_state,
        &TerminalEvent::Key(TerminalKey::CtrlP),
        None,
    );
    // Help -> Status -> MCPs -> Skills -> Models -> Agents
    let _ = dispatch_terminal_event(
        &mut palette_state,
        &TerminalEvent::Key(TerminalKey::Down),
        None,
    );
    let _ = dispatch_terminal_event(
        &mut palette_state,
        &TerminalEvent::Key(TerminalKey::Down),
        None,
    );
    let _ = dispatch_terminal_event(
        &mut palette_state,
        &TerminalEvent::Key(TerminalKey::Down),
        None,
    );
    let _ = dispatch_terminal_event(
        &mut palette_state,
        &TerminalEvent::Key(TerminalKey::Down),
        None,
    );
    let _ = dispatch_terminal_event(
        &mut palette_state,
        &TerminalEvent::Key(TerminalKey::Down),
        None,
    );
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

#[test]
fn test_tab_cycles_agent_in_insert_mode() {
    let mut state = AppState::default();
    // Insert mode is default
    assert_eq!(state.input.mode, InputMode::Insert);
    // Set up 2+ builtin cycle names
    state.status.identity.agent_cycle_names = vec!["planner".to_string(), "maker".to_string()];
    state.set_active_agent_identity("planner");
    // No modals open (default)
    assert_ne!(state.picker.render_kind(), Some(PickerRenderKind::Agent));
    assert_ne!(state.picker.render_kind(), Some(PickerRenderKind::Model));

    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Tab), None);

    assert!(changed);
    // Tab should have queued a cycle request, resulting in Noop + force_changed
    let request = state.take_next_switch_request();
    assert_eq!(request, Some(SwitchRequest::Agent("maker".to_string())));
}

#[test]
fn test_tab_does_not_cycle_in_normal_mode() {
    let mut state = AppState::default();
    state.enter_normal_mode();
    state.status.identity.agent_cycle_names = vec!["planner".to_string(), "maker".to_string()];
    state.set_active_agent_identity("planner");

    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Tab), None);

    assert!(changed);
    // In normal mode, Tab maps to FocusPaneRight, NOT agent cycling
    // Verify no agent switch was queued
    assert_eq!(state.take_next_switch_request(), None);
    // Focus should have cycled
    assert_eq!(state.scroll.pane_focus, crate::state::PaneFocus::Input);
}

#[test]
fn test_tab_does_not_cycle_when_no_builtins() {
    let mut state = AppState::default();
    assert_eq!(state.input.mode, InputMode::Insert);
    // Empty cycle names
    state.status.identity.agent_cycle_names = Vec::new();

    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Tab), None);

    // Tab should pass through — no agent switch queued
    assert_eq!(state.take_next_switch_request(), None);
    // In idle insert mode, Tab maps to CompleteForward which is a no-op in the reducer
    // (no slash menu open, no special handling), so changed depends on state diff
    let _ = changed; // outcome is not Noop+true since cycling didn't fire
}

#[test]
fn agent_picker_ctrl_p_moves_selection_up() {
    let mut state = setup_agent_picker_open();

    // Move down first
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    assert_eq!(state.picker.active_state().unwrap().selection, 1);

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);

    assert!(changed);
    assert_eq!(state.picker.active_state().unwrap().selection, 0);
    assert_eq!(state.picker.render_kind(), Some(PickerRenderKind::Agent));
}
