//! Rows 1-2: the idle mode transitions and the insert-exit chord.

use super::*;

// row 1 — Esc leaves insert mode while idle; nothing arms.
#[test]
fn test_input_matrix_escape_leaves_insert_mode_when_idle() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    assert_eq!(state.input.mode, InputMode::Insert);

    // -- Exec
    let changed = fold_key(&mut state, Key::Esc, None);

    // -- Check
    assert!(changed);
    assert_eq!(state.input.mode, InputMode::Normal);
    assert_eq!(state.scroll.pane_focus, PaneFocus::Transcript);
    assert!(!state.abort.pending);
    assert!(state.status.message.status_line().is_empty());
    Ok(())
}

// row 2 — the jk chord exits insert mode inside its window and expires after it.
#[test]
fn test_input_matrix_insert_exit_chord_exits_and_expires() -> Result<()> {
    // -- Setup & Fixtures: the production window.
    let mut state = AppState::default();

    // -- Exec: the first j arms the chord.
    let armed = fold_key(&mut state, Key::Char('j'), None);

    // -- Check: arming reports no repaint and keeps the mode.
    assert!(!armed);
    assert_eq!(state.input.mode, InputMode::Insert);

    // -- Exec: the second j inside the window.
    fold_key(&mut state, Key::Char('j'), None);

    // -- Check
    assert_eq!(state.input.mode, InputMode::Normal);

    // -- Setup & Fixtures: a window that has already run out.
    let mut expired = AppState {
        input: InputState::default().with_insert_exit_expiry(Duration::ZERO),
        ..Default::default()
    };
    fold_key(&mut expired, Key::Char('j'), None);

    // -- Exec & Check: the second j arrives after the window; no chord fires.
    fold_key(&mut expired, Key::Char('j'), None);
    assert_eq!(expired.input.mode, InputMode::Insert);
    Ok(())
}
