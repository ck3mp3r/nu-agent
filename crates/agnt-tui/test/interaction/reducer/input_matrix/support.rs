//! The matrix's shared harness: a transcript surface, the key and wheel folds,
//! and the active picker's query.

use super::*;

/// A surface over a rendered transcript: `total_visual_rows` rows, the cursor at
/// `cursor_visual_row`, the viewport at `scroll_offset`, and a locked turn
/// behind them when `locked`.
pub(super) fn transcript_state(
    locked: bool,
    mode: InputMode,
    total_visual_rows: usize,
    cursor_visual_row: usize,
    scroll_offset: usize,
) -> AppState {
    let mut state = if locked {
        busy_state_with_clean_transcript()
    } else {
        AppState::default()
    };
    for i in 0..total_visual_rows {
        push_user_line(&mut state, format!("line {i}"));
    }
    state.scroll.viewport_height = 10;
    state.scroll.total_visual_rows = total_visual_rows;
    state.scroll.cursor_visual_row = cursor_visual_row;
    state.scroll.scroll_offset = scroll_offset;
    state.scroll.following_tail = false;
    state.scroll.transcript_area = Rect::new(0, 0, 80, 40);
    if mode == InputMode::Insert {
        state.enter_insert_mode();
    } else {
        state.enter_normal_mode();
    }
    state
}

/// Fold one key press through the reducer, with a cancel observer when given.
pub(super) fn fold_key(
    state: &mut AppState,
    key: Key,
    controller: Option<&CancelController>,
) -> bool {
    reduce(state, UiInput::KeyPressed { key }, controller)
}

/// Fold one wheel notch through the reducer.
pub(super) fn fold_wheel(state: &mut AppState, up: bool, column: u16, row: u16) -> bool {
    reduce(state, UiInput::WheelScrolled { up, column, row }, None)
}

/// The active picker's filter query.
pub(super) fn picker_query(state: &AppState) -> Result<String> {
    let picker = state
        .picker
        .active_state()
        .ok_or("a picker should be open")?;
    Ok(picker.query.clone())
}
