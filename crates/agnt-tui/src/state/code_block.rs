//! Wrap-budget and margin-row utilities shared by the transcript renderer and
//! the visual-row accounting in [`TranscriptStore`].
//!
//! The renderer decides fills from the Block's own `fill` field; the
//! row-accounting path ([`crate::tui_renderer::measure`]) counts the same
//! margin rows so `total_visual_rows` matches the rendered output. The margin
//! row *count* is owned by [`agnt_core::transcript::ir::Fill`]
//! (`margin_row_count`); this module owns the shared wrap-width budget so the
//! renderer and the accounting derive it identically.

/// Right margin kept clear inside every transcript entry: the wrap budget
/// stops this many columns short of the list area's right edge, so no rendered
/// text reaches the last two columns (task 45b99a84).
pub const RIGHT_MARGIN: usize = 2;

/// Width of the status indicator span (icon char + trailing space) appended to
/// row 0 when a status is present.
pub const STATUS_INDICATOR_WIDTH: usize = 2;

/// Effective wrap budget for a ContentLine's text given the pane width, whether
/// a status indicator is rendered on row 0, and the block's lane prefix width
/// ([`crate::tui_renderer::lane_prefix_width`]). Both the renderer and the
/// visual-row accounting derive their wrap width from this so row counts agree
/// by construction: the prefix and the indicator occupy columns on row 0, and
/// the right margin stays clear, so the content budget must shrink by all three
/// or the row overflows the pane and ratatui re-wraps it into an extra visual
/// row the accounting misses.
pub fn content_wrap_width(width: usize, has_status: bool, prefix_width: usize) -> usize {
    let indicator = if has_status {
        STATUS_INDICATOR_WIDTH
    } else {
        0
    };
    width
        .saturating_sub(prefix_width + RIGHT_MARGIN + indicator)
        .max(1)
}
