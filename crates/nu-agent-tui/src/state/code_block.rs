//! Wrap-budget and margin-row utilities shared by the transcript renderer and
//! the visual-row accounting in [`TranscriptStore`].
//!
//! The renderer decides fills from the Block's own `fill` field; the
//! row-accounting path ([`crate::tui_renderer::measure`]) counts the same
//! margin rows so `total_visual_rows` matches the rendered output. The margin
//! row *count* is owned by [`nu_agent_core::transcript::ir::Fill`]
//! (`margin_row_count`); this module owns the shared wrap-width budget so the
//! renderer and the accounting derive it identically.

use crate::tui_renderer::lane_prefix_width;

/// Width of the status indicator span (icon char + trailing space) appended to
/// row 0 when a status is present.
pub const STATUS_INDICATOR_WIDTH: usize = 2;

/// Effective wrap budget for a ContentLine's text given the pane width and
/// whether a status indicator is rendered on row 0. Both the renderer and the
/// visual-row accounting must derive their wrap width from this so row counts
/// agree by construction: the indicator occupies 2 columns on row 0, so the
/// content budget must shrink by that much or the row overflows the pane and
/// ratatui re-wraps it into an extra visual row the accounting misses.
pub fn content_wrap_width(width: usize, has_status: bool) -> usize {
    let indicator = if has_status {
        STATUS_INDICATOR_WIDTH
    } else {
        0
    };
    width.saturating_sub(lane_prefix_width() + indicator).max(1)
}
