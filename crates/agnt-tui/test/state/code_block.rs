use agnt_core::transcript::ir::Fill;

use super::code_block::content_wrap_width;
use crate::tui_renderer::lane_prefix_width;

#[test]
fn content_wrap_width_shrinks_by_lane_prefix_and_indicator() {
    // -- Setup & Fixtures: (width, with_status, expected)
    let cases = [
        (120usize, false, 120 - lane_prefix_width()),
        (120, true, 120 - lane_prefix_width() - 2),
        (1, true, 1),
    ];

    // -- Exec & Check
    for (width, with_status, expected) in cases {
        assert_eq!(
            content_wrap_width(width, with_status),
            expected,
            "width={width} with_status={with_status}"
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
