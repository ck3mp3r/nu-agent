//! Rows 6, 22, 23: the lock gate — composing through it, and which surface owns
//! the keys it refuses.

use super::*;

// row 6 — a locked char composes: the fold refuses and the chord never arms.
#[test]
fn test_input_matrix_locked_insert_compose_leaves_keymap_untouched() -> Result<()> {
    // -- Setup & Fixtures: a locked turn in insert mode with no modal picker.
    let mut state = busy_state_with_clean_transcript();
    assert!(state.input_locked);
    assert!(state.picker.active().is_none());

    // -- Exec: a plain character, then the chord's own key.
    let plain = fold_key(&mut state, Key::Char('a'), None);
    let chord = fold_key(&mut state, Key::Char('j'), None);

    // -- Check: the coordinator's textarea gets both; no keymap state moves.
    assert!(!plain);
    assert!(!chord);
    assert_eq!(state.input.mode, InputMode::Insert);
    assert!(!state.input.insert_exit_pending_j());
    assert_eq!(state.input.pending_submit_text, None);
    assert!(pending_prompt_ids(&state).is_empty());
    Ok(())
}

// row 22 — a modal picker edits its own query under the lock, not the draft.
#[test]
fn test_input_matrix_locked_palette_edits_its_query() -> Result<()> {
    // -- Setup & Fixtures: a locked turn with the command palette up.
    let mut state = busy_state_with_clean_transcript();
    state.picker.open(ActivePicker::CommandPalette);

    // -- Exec: two chars and a backspace land on the popup.
    fold_key(&mut state, Key::Char('h'), None);
    fold_key(&mut state, Key::Char('e'), None);
    let typed = picker_query(&state)?;
    fold_key(&mut state, Key::Backspace, None);
    let erased = picker_query(&state)?;

    // -- Check: the filter moved, the draft did not, the mode held.
    assert_eq!(typed, "he");
    assert_eq!(erased, "h");
    assert_eq!(state.input.pending_submit_text, None);
    assert_eq!(state.input.mode, InputMode::Insert);
    Ok(())
}

// row 23 — the inline slash list keeps its query and the chord unarmed.
#[test]
fn test_input_matrix_locked_inline_slash_keeps_its_query() -> Result<()> {
    // -- Setup & Fixtures: a locked turn with the composer's own list up.
    let mut state = busy_state_with_clean_transcript();
    state.picker.open(ActivePicker::InlineSlash);

    // -- Exec: the gate sees the character before the lens does.
    let changed = fold_key(&mut state, Key::Char('h'), None);

    // -- Check: the gate refused the fold, so neither surface moved.
    assert!(!changed);
    assert_eq!(picker_query(&state)?, "");
    assert!(!state.input.insert_exit_pending_j());
    assert_eq!(state.input.mode, InputMode::Insert);
    Ok(())
}
