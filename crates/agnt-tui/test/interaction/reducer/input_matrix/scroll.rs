//! Rows 5, 8, 9: the normal-mode scroll keys — under the lock and while idle.

use super::*;

// row 5 — j and k scroll while the turn is locked in normal mode.
#[test]
fn test_input_matrix_locked_normal_jk_scrolls() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = transcript_state(true, InputMode::Normal, 30, 10, 5);

    // -- Exec & Check: line down, then line up.
    assert!(fold_key(&mut state, Key::Char('j'), None));
    assert_eq!(state.scroll.cursor_visual_row, 11);
    assert!(fold_key(&mut state, Key::Char('k'), None));
    assert_eq!(state.scroll.cursor_visual_row, 10);
    Ok(())
}

// row 8 — gg, G and the paging keys scroll while the turn is locked.
#[test]
fn test_input_matrix_locked_normal_navigation_scrolls() -> Result<()> {
    // -- Exec & Check: the paging keys page one page each way.
    for key in [Key::PageUp, Key::CtrlU] {
        let mut state = transcript_state(true, InputMode::Normal, 30, 10, 5);
        assert!(fold_key(&mut state, key, None), "{key:?}");
        assert_eq!(state.scroll.cursor_visual_row, 2, "{key:?}");
    }
    for key in [Key::PageDown, Key::CtrlD] {
        let mut state = transcript_state(true, InputMode::Normal, 30, 10, 5);
        assert!(fold_key(&mut state, key, None), "{key:?}");
        assert_eq!(state.scroll.cursor_visual_row, 18, "{key:?}");
    }

    // -- Setup & Fixtures: the pending gg chord, then G.
    let mut state = transcript_state(true, InputMode::Normal, 30, 10, 5);

    // -- Exec & Check: gg jumps to the top.
    fold_key(&mut state, Key::Char('g'), None);
    assert!(fold_key(&mut state, Key::Char('g'), None));
    assert_eq!(state.scroll.cursor_visual_row, 0);
    assert_eq!(state.scroll.scroll_offset, 0);

    // -- Exec & Check: G jumps to the last visual row and follows the tail.
    assert!(fold_key(&mut state, Key::Char('G'), None));
    assert_eq!(state.scroll.cursor_visual_row, 29);
    assert!(state.scroll.following_tail);
    Ok(())
}

// row 9 — idle normal jk moves the cursor and the viewport.
#[test]
fn test_input_matrix_normal_jk_moves_cursor_and_viewport() -> Result<()> {
    // -- Setup & Fixtures: the cursor two rows above the bottom margin.
    let mut state = transcript_state(false, InputMode::Normal, 20, 6, 0);

    // -- Exec & Check: the cursor leaves the margin, so the viewport follows.
    assert!(fold_key(&mut state, Key::Char('j'), None));
    assert_eq!(state.scroll.cursor_visual_row, 7);
    assert_eq!(state.scroll.scroll_offset, 1);
    assert!(!state.scroll.following_tail);

    // -- Exec & Check: the cursor moves back; the viewport holds.
    assert!(fold_key(&mut state, Key::Char('k'), None));
    assert_eq!(state.scroll.cursor_visual_row, 6);
    assert_eq!(state.scroll.scroll_offset, 1);
    Ok(())
}
