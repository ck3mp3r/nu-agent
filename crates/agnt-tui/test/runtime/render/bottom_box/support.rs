//! Shared fixtures for the bottom-box render test sub-files.
//!
//! Each sub-file pulls these in with `use super::bottom_box_support::*;`.

pub(super) use crate::runtime::test_driver::RenderLoopDriver;

use ratatui::style::Color;

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Background of the cell at `(x, y)` in the last rendered frame.
pub(super) fn cell_bg(driver: &RenderLoopDriver, x: u16, y: u16) -> Option<Color> {
    driver.cell_style_at(x, y).bg
}

/// Row index of the input box top border. A frame with no popup open paints
/// exactly one `╭`.
pub(super) fn input_top_row(driver: &RenderLoopDriver) -> Result<u16> {
    driver
        .buffer_text()
        .lines()
        .position(|line| line.contains('╭'))
        .map(|row| row as u16)
        .ok_or_else(|| "should find the input box top border row".into())
}
