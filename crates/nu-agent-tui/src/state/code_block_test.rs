use nu_agent_core::transcript::ir::Fill;

use super::code_block::content_wrap_width;
use crate::tui_renderer::lane_prefix_width;

// ── content_wrap_width ──────────────────────────────────────────────────────

#[test]
fn content_wrap_width_shrinks_by_lane_prefix_and_indicator() {
    let plain = content_wrap_width(120, false);
    assert_eq!(plain, 120 - lane_prefix_width());

    let with_status = content_wrap_width(120, true);
    assert_eq!(with_status, 120 - lane_prefix_width() - 2);
}

#[test]
fn content_wrap_width_floor_is_one() {
    assert_eq!(content_wrap_width(1, true), 1);
}

// ── Fill::margin_row_count (task 69bd5698) ──────────────────────────────────

#[test]
fn fill_code_margin_row_count_is_two() {
    assert_eq!(
        Fill::Code.margin_row_count(),
        2,
        "one top + one bottom margin row"
    );
}

#[test]
fn fill_full_and_none_have_no_margin_rows() {
    assert_eq!(Fill::Full.margin_row_count(), 0);
    assert_eq!(Fill::None.margin_row_count(), 0);
}
