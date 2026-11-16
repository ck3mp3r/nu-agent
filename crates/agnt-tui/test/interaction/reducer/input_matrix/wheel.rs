//! Rows 3, 4, 10: the wheel notch — its mode, the typing that follows it, and
//! its fold size and hit test.

use super::*;

// row 3 — a wheel notch keeps insert mode and the input focus.
#[test]
fn test_input_matrix_wheel_keeps_insert_mode_and_input_focus() -> Result<()> {
    // -- Setup & Fixtures: a locked turn in insert mode, cursor at the top.
    let mut state = transcript_state(true, InputMode::Insert, 30, 0, 0);

    // -- Exec
    let changed = fold_wheel(&mut state, false, 5, 5);

    // -- Check: the notch scrolls; the surface stays a composer.
    assert!(changed);
    assert_eq!(state.input.mode, InputMode::Insert);
    assert_eq!(state.scroll.pane_focus, PaneFocus::Input);
    assert_eq!(state.scroll.cursor_visual_row, 3);
    Ok(())
}

// row 4 — typing after a notch keeps the draft and the insert glyph.
#[test]
fn test_input_matrix_typing_after_wheel_stays_in_insert() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = transcript_state(true, InputMode::Insert, 30, 0, 0);
    fold_wheel(&mut state, false, 5, 5);

    // -- Exec: the composer's key follows the notch.
    let changed = fold_key(&mut state, Key::Char('a'), None);

    // -- Check: the reducer leaves the character to the coordinator's textarea,
    // and insert mode keeps the bottom box painting the `❯` indicator.
    assert!(!changed);
    assert_eq!(state.input.mode, InputMode::Insert);
    assert_eq!(state.scroll.pane_focus, PaneFocus::Input);
    Ok(())
}

// row 10 — a notch folds three lines and honours the transcript hit test.
#[test]
fn test_input_matrix_wheel_folds_three_lines_and_hit_tests() -> Result<()> {
    // -- Setup & Fixtures: the notch lands inside the transcript area.
    let mut state = transcript_state(false, InputMode::Normal, 30, 0, 0);

    // -- Exec & Check: three visual lines per notch, the viewport unmoved.
    assert!(fold_wheel(&mut state, false, 5, 5));
    assert_eq!(state.scroll.cursor_visual_row, 3);
    assert_eq!(state.scroll.scroll_offset, 0);

    // -- Setup & Fixtures: an area the render reported away from the origin.
    state.scroll.transcript_area = Rect::new(10, 2, 40, 10);

    // -- Exec & Check: a point left of the area is rejected.
    assert!(!fold_wheel(&mut state, false, 9, 5));
    assert_eq!(state.scroll.cursor_visual_row, 3);

    // -- Exec & Check: the area's own top-left cell folds.
    assert!(fold_wheel(&mut state, false, 10, 2));
    assert_eq!(state.scroll.cursor_visual_row, 6);
    Ok(())
}
