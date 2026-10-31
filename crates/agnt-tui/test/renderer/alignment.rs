//! Transcript column alignment: every lane pins a fixed gutter, and the wrap
//! budget keeps the pane's last two columns clear.
//!
//! Coordinates are pane-local: 0 is the pane's first column. The tool,
//! compaction and system lanes paint `[cursor slot][marker]` with text at 4; the
//! user lane paints the dotted rail at 0 with text at 2; the assistant lane
//! paints a blank two-column prefix with text at 2; the banner centers its row.

use super::support::*;
use agnt_core::transcript::ir::{MessageRole, NoticeKind};
use agnt_core::transcript::renderer::ItemStatus;
use agnt_render::catalog::LaneKind;

// region:    --- Lane gutters

/// WHEN a tool, compaction or system entry wraps, THE marker SHALL sit at column
/// 2 with text at 4 on row 0, and every continuation row SHALL keep the blank
/// marker slot with text at 4.
#[test]
fn test_renderer_alignment_marker_lanes_keep_marker_at_two_and_text_at_four() -> Result<()> {
    // -- Setup & Fixtures
    let cases = [
        (
            tool_entry("read", &long_prose(), None, None, ItemStatus::Done),
            '\u{2699}',
            LaneKind::Tool,
        ),
        (
            notice_entry(NoticeKind::Compaction, &long_prose()),
            '~',
            LaneKind::Compaction,
        ),
        (
            notice_entry(NoticeKind::System, &long_prose()),
            '\u{00b7}',
            LaneKind::System,
        ),
    ];

    // -- Exec & Check
    for (entry, marker, lane) in cases {
        let backend = render(&entry)?;
        let rows = measured_rows(&entry)?;
        assert!(rows >= 2, "lane {lane:?} must wrap; got {rows}");
        let row0 = row_text(&backend, 0);
        assert_eq!(
            row0.chars().nth(2),
            Some(marker),
            "lane {lane:?} must paint its marker at column 2; got {row0:?}"
        );
        assert_ne!(
            row0.chars().nth(4),
            Some(' '),
            "lane {lane:?} must start its text at column 4; got {row0:?}"
        );
        for y in 1..u16::try_from(rows)? {
            let row = row_text(&backend, y);
            assert!(
                row.starts_with("    ") && row.chars().nth(4) != Some(' '),
                "lane {lane:?} continuation row {y} must keep the blank marker slot; got {row:?}"
            );
            assert!(
                !row.contains(marker),
                "lane {lane:?} continuation row {y} must blank the marker; got {row:?}"
            );
        }
    }
    Ok(())
}

/// WHEN a user message wraps, THE dotted rail SHALL sit at column 0 on the row
/// and on every continuation row, with text at 2.
#[test]
fn test_renderer_alignment_user_rows_carry_rail_at_zero_and_text_at_two() -> Result<()> {
    // -- Setup & Fixtures
    let entry = message_entry(MessageRole::User, &long_prose());

    // -- Exec
    let backend = render(&entry)?;

    // -- Check
    let rows = measured_rows(&entry)?;
    assert!(rows >= 2, "the fixture must wrap; got {rows}");
    for y in 0..u16::try_from(rows)? {
        let row = row_text(&backend, y);
        assert!(
            row.starts_with("\u{254e} "),
            "user row {y} must carry the rail at column 0; got {row:?}"
        );
        assert_ne!(
            row.chars().nth(2),
            Some(' '),
            "user row {y} must start its text at column 2; got {row:?}"
        );
    }
    Ok(())
}

/// WHEN an assistant message wraps, THE text SHALL start at column 2 on the row
/// and on every continuation row.
#[test]
fn test_renderer_alignment_assistant_rows_start_text_at_two() -> Result<()> {
    // -- Setup & Fixtures
    let entry = message_entry(MessageRole::Assistant, &long_prose());

    // -- Exec
    let backend = render(&entry)?;

    // -- Check
    let rows = measured_rows(&entry)?;
    assert!(rows >= 2, "the fixture must wrap; got {rows}");
    for y in 0..u16::try_from(rows)? {
        let row = row_text(&backend, y);
        assert!(
            row.starts_with("  ") && row.chars().nth(2) != Some(' '),
            "assistant row {y} must start its text at column 2; got {row:?}"
        );
    }
    Ok(())
}

/// WHEN a banner renders, THE text SHALL be centered: the gutter plus the text
/// is centered as one row, with the odd column left of the glyphs.
#[test]
fn test_renderer_alignment_banner_is_centered() -> Result<()> {
    // -- Setup & Fixtures
    let entry = banner_entry("NUAGENT");

    // -- Exec
    let backend = render(&entry)?;

    // -- Check: the banner lane reserves a four-column gutter, so seven glyphs
    // centered in eighty columns start at 34 + 4 = 38 and leave 35 columns right.
    let row = row_text(&backend, 0);
    let glyph_at = row.find("NUAGENT").ok_or("the banner must render")?;
    let trailing = row.chars().rev().take_while(|c| *c == ' ').count();
    assert_eq!(glyph_at, 38, "the banner must center; got {row:?}");
    assert_eq!(
        trailing, 35,
        "the right padding must mirror the left; got {row:?}"
    );
    assert_eq!(glyph_at + 7 + trailing, usize::from(WIDTH));
    Ok(())
}

// endregion: --- Lane gutters

// region:    --- Right margin

/// WHEN any entry lane wraps, THE last painted column SHALL stay inside
/// `WIDTH - 2`: the wrap budget keeps the pane's last two columns clear.
#[test]
fn test_renderer_alignment_every_lane_keeps_the_right_margin_clear() -> Result<()> {
    // -- Setup & Fixtures: one wrapping entry per lane.
    let cases = [
        message_entry(MessageRole::User, &long_prose()),
        message_entry(MessageRole::Assistant, &long_prose()),
        tool_entry("read", &long_prose(), None, None, ItemStatus::Done),
        notice_entry(NoticeKind::System, &long_prose()),
        notice_entry(NoticeKind::Compaction, &long_prose()),
    ];

    // -- Exec & Check
    for entry in cases {
        let backend = render(&entry)?;
        let rows = measured_rows(&entry)?;
        assert!(rows >= 2, "the fixture must wrap; got {rows}");
        for y in 0..u16::try_from(rows)? {
            let width = row_trimmed(&backend, y).chars().count();
            assert!(
                width <= usize::from(WIDTH) - 2,
                "row {y} must leave the last two columns clear; got width {width}: {:?}",
                row_trimmed(&backend, y)
            );
        }
    }
    Ok(())
}

/// WHEN the backend measures an entry and paints it, THE painted row count SHALL
/// equal the measured one for every lane — the planner and the pane share one
/// row budget.
#[test]
fn test_renderer_alignment_measured_rows_match_painted_rows() -> Result<()> {
    // -- Setup & Fixtures
    let cases = [
        message_entry(MessageRole::User, &long_prose()),
        message_entry(MessageRole::Assistant, &long_prose()),
        tool_entry("read", &long_prose(), None, None, ItemStatus::Done),
        notice_entry(NoticeKind::System, &long_prose()),
        banner_entry("NUAGENT\nline2"),
    ];

    // -- Exec & Check
    for entry in cases {
        let (backend, rows) = render_at(&entry, 0)?;
        assert_eq!(
            painted_rows(&backend),
            rows,
            "the measured rows must equal the painted rows; painted {:?}",
            row_trimmed(&backend, 0)
        );
    }
    Ok(())
}

// endregion: --- Right margin
