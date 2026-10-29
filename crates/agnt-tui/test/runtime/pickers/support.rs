//! Shared imports for the runtime picker test modules.
//!
//! Holds the imports and the helper re-exports every topical picker test file
//! needs. Sub-files pull them in with `use super::*;`.

pub(super) use crate::runtime::render;
pub(super) use crate::runtime::status::help_test::{
    agent_picker_row_cells_for_test, model_picker_row_cells_for_test,
};
pub(super) use crate::runtime::test_driver::RenderLoopDriver;
pub(super) use crate::state::{ActivePicker, AppState};

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Test Support

/// The popup border box as rendered in the driver buffer: its `(x, y, width,
/// height)` in terminal cells.
pub(super) fn rendered_popup_rect(driver: &RenderLoopDriver) -> Option<(u16, u16, u16, u16)> {
    let text = driver.buffer_text();
    let rows = text.lines().collect::<Vec<_>>();
    let (top_y, top_row) = rows
        .iter()
        .enumerate()
        .find_map(|(y, row)| row.contains('╭').then_some((y, *row)))?;
    let left = top_row.chars().position(|ch| ch == '╭')?;
    let right = top_row.chars().position(|ch| ch == '╮')?;
    let bottom_y = rows
        .iter()
        .enumerate()
        .skip(top_y + 1)
        .find_map(|(y, row)| (row.chars().nth(left) == Some('╰')).then_some(y))?;
    Some((
        u16::try_from(left).ok()?,
        u16::try_from(top_y).ok()?,
        u16::try_from(right - left + 1).ok()?,
        u16::try_from(bottom_y - top_y + 1).ok()?,
    ))
}

// endregion: --- Test Support
