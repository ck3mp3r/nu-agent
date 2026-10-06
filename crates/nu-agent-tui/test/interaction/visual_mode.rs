use super::*;

#[test]
fn normal_v_key_enters_visual_mode_and_j_yank_works() -> Result<()> {
    let mut state = AppState::default();
    state.enter_normal_mode();
    state.scroll.cursor_visual_row = 0;
    state.scroll.total_visual_rows = 5;
    state.scroll.entry_indices = (0..5).collect();

    // Press 'v' → should enter Visual mode and set a selection
    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('v')),
        None,
    );
    assert_eq!(state.input.mode, InputMode::Visual);
    let sel = state
        .scroll
        .selection
        .ok_or("should have transcript selection after pressing v")?;
    assert_eq!(sel.anchor(), 0);
    assert_eq!(sel.cursor(), 0);

    // Press 'j' → should extend selection down
    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('j')),
        None,
    );
    let sel = state
        .scroll
        .selection
        .ok_or("should still have transcript selection after pressing j")?;
    assert_eq!(sel.cursor(), 1);

    // Populate rendered lines and press 'y' → should yank selected rows
    state.scroll.rendered_line_text = vec![
        "line 0".to_string(),
        "line 1".to_string(),
        "line 2".to_string(),
    ];
    state.scroll.rendered_line_start_row = 0;
    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('y')),
        None,
    );

    let clipboard = state.input.take_clipboard_request();
    assert_eq!(clipboard, Some("line 0\nline 1".to_string()));
    assert!(state.scroll.selection.is_none());
    assert_eq!(state.input.mode, InputMode::Normal);
    Ok(())
}
