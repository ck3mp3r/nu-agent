use super::*;

#[test]
fn ctrl_p_opens_palette_and_second_ctrl_p_moves_selection_up() {
    let mut state = AppState::default();

    let opened = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    assert!(opened);
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );

    // Move down so there's somewhere to go up
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    assert_eq!(state.picker.active_state().unwrap().selection, 1);

    // Ctrl-P while palette open moves selection up (does not close)
    let moved_up =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    assert!(moved_up);
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
    assert_eq!(state.picker.active_state().unwrap().selection, 0);

    // Esc closes the palette
    let closed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Esc), None);
    assert!(closed);
    assert_ne!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
}

#[test]
fn escape_closes_palette_only_and_preserves_insert_mode() {
    let mut state = AppState::default();
    assert_eq!(state.input.mode, InputMode::Insert);
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );

    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Esc), None);
    assert!(changed);
    assert_ne!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
    assert_eq!(state.input.mode, InputMode::Insert);
}

#[test]
fn palette_navigation_supports_arrows_and_ctrl_np_and_enter_routes_action() {
    let mut state = AppState::default();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    assert_eq!(state.picker.active_state().unwrap().selection, 1);

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    assert_eq!(state.picker.active_state().unwrap().selection, 0);

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlN), None);
    assert_eq!(state.picker.active_state().unwrap().selection, 1);

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
    assert_eq!(state.info_panel, Some(InfoPanel::Status));
    assert_ne!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
}

#[test]
fn palette_selection_can_open_mcps_panel() {
    let mut state = AppState::default();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);

    // Help -> Status -> MCPs
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    let selected = state.picker.active_state().unwrap().selected().unwrap();
    assert_eq!(selected.id, "MCPs");

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
    assert_eq!(state.info_panel, Some(InfoPanel::Mcps));
    assert_ne!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
}

#[test]
fn palette_selection_can_open_skills_panel() {
    let mut state = AppState::default();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);

    // Help -> Status -> MCPs -> Skills
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    let selected = state.picker.active_state().unwrap().selected().unwrap();
    assert_eq!(selected.id, "Skills");

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
    assert_eq!(state.info_panel, Some(InfoPanel::Skills));
    assert_ne!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
}

#[test]
fn palette_escape_closes_without_panel_route_regression() {
    let mut state = AppState::default();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );

    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Esc), None);
    assert!(changed);
    assert_ne!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
    assert_eq!(state.info_panel, None);
}

#[test]
fn command_palette_j_feeds_query_not_navigation() {
    let mut state = AppState::default();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('j')),
        None,
    );

    assert!(changed);
    assert_eq!(state.picker.active_state().unwrap().query, "j");
    // Selection should not have changed (still at 0)
    assert_eq!(state.picker.active_state().unwrap().selection, 0);
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
}

#[test]
fn command_palette_k_feeds_query_not_navigation() {
    let mut state = AppState::default();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('k')),
        None,
    );

    assert!(changed);
    assert_eq!(state.picker.active_state().unwrap().query, "k");
    assert_eq!(state.picker.active_state().unwrap().selection, 0);
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
}

#[test]
fn command_palette_ctrl_n_moves_selection_down() {
    let mut state = AppState::default();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
    assert_eq!(state.picker.active_state().unwrap().selection, 0);

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlN), None);

    assert!(changed);
    assert_eq!(state.picker.active_state().unwrap().selection, 1);
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
}

#[test]
fn command_palette_ctrl_p_moves_selection_up() {
    let mut state = AppState::default();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );

    // Move down first so we have somewhere to go up
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    assert_eq!(state.picker.active_state().unwrap().selection, 1);

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);

    assert!(changed);
    assert_eq!(state.picker.active_state().unwrap().selection, 0);
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
}

#[test]
fn ctrl_p_with_no_modal_open_opens_command_palette() {
    let mut state = AppState::default();
    assert_ne!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
    assert_ne!(state.picker.render_kind(), Some(PickerRenderKind::Model));
    assert_ne!(state.picker.render_kind(), Some(PickerRenderKind::Agent));

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);

    assert!(changed);
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
}

#[test]
fn palette_filters_with_non_prefix_query_before_enter_routes_help() {
    let mut state = AppState::default();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);

    let _ = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('h')),
        None,
    );
    let _ = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('p')),
        None,
    );
    let ids: Vec<String> = state
        .picker
        .active_state()
        .unwrap()
        .filtered()
        .iter()
        .map(|o| o.id.clone())
        .collect();
    assert_eq!(ids, vec!["Help"]);

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
    assert_eq!(state.info_panel, Some(InfoPanel::Help));
    assert_ne!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
}
