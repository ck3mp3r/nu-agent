use super::*;

#[test]
fn command_palette_navigation_table() {
    // Ctrl-P opens the palette; a second Ctrl-P moves the selection up.
    let mut state = AppState::default();
    let opened = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    assert!(opened);
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    assert_eq!(state.picker.active_state().unwrap().selection, 1);
    let moved_up =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    assert!(moved_up);
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
    assert_eq!(state.picker.active_state().unwrap().selection, 0);
    let closed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Esc), None);
    assert!(closed);
    assert_ne!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );

    // Ctrl-N moves the selection down.
    let mut state = AppState::default();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    assert_eq!(state.picker.active_state().unwrap().selection, 0);
    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlN), None);
    assert!(changed);
    assert_eq!(state.picker.active_state().unwrap().selection, 1);
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );

    // Ctrl-P moves the selection up.
    let mut state = AppState::default();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
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
fn command_palette_query_feed_table() {
    for ch in ['j', 'k'] {
        let mut state = AppState::default();
        let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
        assert_eq!(
            state.picker.render_kind(),
            Some(PickerRenderKind::CommandPalette)
        );

        let changed =
            dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Char(ch)), None);

        assert!(changed);
        assert_eq!(state.picker.active_state().unwrap().query, ch.to_string());
        assert_eq!(state.picker.active_state().unwrap().selection, 0);
        assert_eq!(
            state.picker.render_kind(),
            Some(PickerRenderKind::CommandPalette)
        );
    }
}

#[test]
fn command_palette_escape_table() {
    // Escape closes the palette only and preserves insert mode.
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

    // Escape closes without a panel route regression.
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
fn command_palette_routing_table() {
    // Arrows and Ctrl-N/P navigate; Enter routes the selected action.
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

    // The selection can open the MCPs panel.
    let mut state = AppState::default();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
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

    // The selection can open the Skills panel.
    let mut state = AppState::default();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    for _ in 0..3 {
        let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    }
    let selected = state.picker.active_state().unwrap().selected().unwrap();
    assert_eq!(selected.id, "Skills");
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
    assert_eq!(state.info_panel, Some(InfoPanel::Skills));
    assert_ne!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );

    // Ctrl-P with no modal open opens the command palette.
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

    // A non-prefix query filters before Enter routes Help.
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
