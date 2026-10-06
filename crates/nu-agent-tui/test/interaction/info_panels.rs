use super::*;

#[test]
fn escape_closes_info_panel_without_mode_regression() {
    let mut state = AppState::default();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
    assert_eq!(state.info_panel, Some(InfoPanel::Help));
    assert_eq!(state.input.mode, InputMode::Insert);

    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Esc), None);
    assert!(changed);
    assert_eq!(state.info_panel, None);
    assert_eq!(state.input.mode, InputMode::Insert);
}

#[test]
fn help_panel_ctrl_n_scrolls_down() {
    let mut state = AppState::default();
    state.open_info_panel(crate::state::InfoPanel::Help);
    assert_eq!(state.info_panel_scroll, 0);

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlN), None);

    assert!(changed);
    assert_eq!(state.info_panel_scroll, 1);
}

#[test]
fn help_panel_ctrl_p_scrolls_up() {
    let mut state = AppState::default();
    state.open_info_panel(crate::state::InfoPanel::Help);
    state.info_panel_scroll = 3;

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);

    assert!(changed);
    assert_eq!(state.info_panel_scroll, 2);
}

#[test]
fn help_panel_j_is_noop() {
    let mut state = AppState::default();
    state.open_info_panel(crate::state::InfoPanel::Help);
    state.info_panel_scroll = 0;

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('j')),
        None,
    );

    // j is not a navigation key in the help panel — scroll must not change
    assert_eq!(state.info_panel_scroll, 0);
    // changed may be true or false depending on reducer noop handling;
    // the important assertion is that scroll did not increment
    let _ = changed;
}
