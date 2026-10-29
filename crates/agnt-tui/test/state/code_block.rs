use agnt_core::transcript::ir::Fill;
use agnt_core::transcript::ir::Lane;

use super::code_block::{RIGHT_MARGIN, content_wrap_width};
use crate::tui_renderer::lane_prefix_width;

#[test]
fn content_wrap_width_shrinks_by_lane_prefix_margin_and_indicator() {
    // -- Setup & Fixtures: user/assistant lanes carry a 2-col prefix, tool,
    // compaction, and system lanes a 4-col prefix.
    let user_prefix = lane_prefix_width(&Lane::Marker("\u{254e}"));
    let tool_prefix = lane_prefix_width(&Lane::Marker("\u{2699}"));
    // (width, with_status, prefix, expected)
    let cases = [
        (120usize, false, user_prefix, 120 - 2 - RIGHT_MARGIN),
        (120, true, user_prefix, 120 - 2 - RIGHT_MARGIN - 2),
        (120, false, tool_prefix, 120 - 4 - RIGHT_MARGIN),
        (120, true, tool_prefix, 120 - 4 - RIGHT_MARGIN - 2),
        (1, true, tool_prefix, 1),
    ];

    // -- Exec & Check
    for (width, with_status, prefix, expected) in cases {
        assert_eq!(
            content_wrap_width(width, with_status, prefix),
            expected,
            "width={width} with_status={with_status} prefix={prefix}"
        );
    }
}

#[test]
fn fill_margin_row_count_matches_variant() {
    // -- Setup & Fixtures: (fill, expected margin rows)
    let cases = [(Fill::Code, 2), (Fill::Full, 0), (Fill::None, 0)];

    // -- Exec & Check
    for (fill, expected) in cases {
        assert_eq!(fill.margin_row_count(), expected, "fill={fill:?}");
    }
}
