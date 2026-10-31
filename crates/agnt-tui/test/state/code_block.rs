use agnt_render::catalog::LaneKind;

use super::code_block::{RIGHT_MARGIN, content_wrap_width};

#[test]
fn content_wrap_width_shrinks_by_lane_prefix_margin_and_indicator() {
    // -- Setup & Fixtures: user prose lanes carry a 2-col prefix, tool and
    // notice lanes a 4-col prefix.
    let user_prefix = LaneKind::User.prefix_width();
    let tool_prefix = LaneKind::Tool.prefix_width();
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
