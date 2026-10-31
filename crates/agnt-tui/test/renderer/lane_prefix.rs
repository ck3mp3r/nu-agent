//! Lane gutters: the marker slot, its spacing, and the rail's continuity.

use super::support::*;
use agnt_core::transcript::ir::{MessageRole, NoticeKind};
use agnt_core::transcript::renderer::ItemStatus;
use agnt_render::catalog::LaneKind;

use crate::widgets::RatatuiBackend;

// region:    --- Marker spacing

/// WHEN a done tool call renders, THE marker and the checkmark SHALL be
/// space-separated, so the gutter reads `⚙ ✓`.
#[test]
fn test_renderer_lane_prefix_tool_marker_and_done_indicator_are_spaced() -> Result<()> {
    // -- Setup & Fixtures
    let entry = bare_tool(ItemStatus::Done);

    // -- Exec
    let backend = render(&entry)?;

    // -- Check
    let row = row_trimmed(&backend, 0);
    assert_eq!(row, "  \u{2699} \u{2713} read \u{2192} a.rs");
    assert!(
        !row.contains("\u{2699}\u{2713}"),
        "the marker and the checkmark must not be adjacent"
    );
    Ok(())
}

/// WHEN an in-progress tool call renders at clock 0, THE marker and the spinner
/// SHALL be space-separated, so the gutter reads `⚙ ⠋`.
#[test]
fn test_renderer_lane_prefix_tool_marker_and_spinner_are_spaced() -> Result<()> {
    // -- Setup & Fixtures: now_millis 0 selects spinner frame 0.
    let entry = bare_tool(ItemStatus::InProgress);

    // -- Exec
    let backend = render(&entry)?;

    // -- Check
    let row = row_trimmed(&backend, 0);
    assert_eq!(row, "  \u{2699} \u{280b} read \u{2192} a.rs");
    assert!(
        !row.contains("\u{2699}\u{280b}"),
        "the marker and the spinner must not be adjacent"
    );
    Ok(())
}

/// THE tool lane's gutter SHALL reserve the two-column cursor slot and the
/// two-column marker slot: the icon sits at column 2 and the content at 4.
#[test]
fn test_renderer_lane_prefix_tool_gutter_is_two_slots_wide() -> Result<()> {
    // -- Setup & Fixtures
    let entry = bare_tool(ItemStatus::Queued);

    // -- Exec
    let backend = render(&entry)?;

    // -- Check
    let row = row_text(&backend, 0);
    assert!(row.starts_with("  \u{2699} "), "got {row:?}");
    assert_eq!(first_glyph_column(&backend, 0), Some(2));
    Ok(())
}

/// WHEN the user, compaction and system lanes render, EACH SHALL paint its own
/// two-character marker: the rail at column 0, the notices after their cursor
/// slot.
#[test]
fn test_renderer_lane_prefix_every_marker_lane_renders_its_marker() -> Result<()> {
    // -- Setup & Fixtures
    let cases = [
        (
            message_entry(MessageRole::User, "hi"),
            "\u{254e} hi",
            LaneKind::User,
        ),
        (
            notice_entry(NoticeKind::Compaction, "c"),
            "  ~ c",
            LaneKind::Compaction,
        ),
        (
            notice_entry(NoticeKind::System, "s"),
            "  \u{00b7} s",
            LaneKind::System,
        ),
        (
            message_entry(MessageRole::Assistant, "hi"),
            "  hi",
            LaneKind::Assistant,
        ),
    ];

    // -- Exec & Check
    for (entry, expected, lane) in cases {
        let backend = render(&entry)?;
        assert_eq!(
            row_trimmed(&backend, 0),
            expected,
            "lane {lane:?} must paint its marker"
        );
        assert_eq!(
            first_glyph_column(&backend, 0).unwrap_or(0),
            u16::try_from(if lane == LaneKind::User { 0 } else { 2 })
                .map_err(|_| "column must fit u16")?,
            "lane {lane:?} must pin its marker column"
        );
    }
    Ok(())
}

/// THE lane prefix width SHALL equal the columns the lane reserves before its
/// content, and row 0 SHALL start painting at its marker — or, on the prose
/// lanes, at the gutter's end.
#[test]
fn test_renderer_lane_prefix_width_matches_reserved_columns() -> Result<()> {
    // -- Setup & Fixtures: one entry per lane, the gutter it must reserve, and the
    // column row 0 starts painting at.
    let cases = [
        (bare_tool(ItemStatus::Done), LaneKind::Tool, 4usize, 2u16),
        (
            notice_entry(NoticeKind::Compaction, "compacted"),
            LaneKind::Compaction,
            4,
            2,
        ),
        (
            notice_entry(NoticeKind::System, "note"),
            LaneKind::System,
            4,
            2,
        ),
        (message_entry(MessageRole::User, "hi"), LaneKind::User, 2, 0),
        (
            message_entry(MessageRole::Assistant, "hi"),
            LaneKind::Assistant,
            2,
            2,
        ),
    ];

    // -- Exec & Check
    for (entry, lane, gutter, first_column) in cases {
        assert_eq!(
            lane.prefix_width(),
            gutter,
            "lane {lane:?} must reserve {gutter} columns"
        );
        let backend = render(&entry)?;
        assert_eq!(
            first_glyph_column(&backend, 0),
            Some(first_column),
            "lane {lane:?} must start painting at column {first_column}"
        );
    }
    Ok(())
}

// endregion: --- Marker spacing

// region:    --- Continuation rows

/// WHEN a user message holds several content lines, THE dotted rail `╎` SHALL
/// sit at column 0 on every row and THE text SHALL start at column 2.
#[test]
fn test_renderer_lane_prefix_user_multi_line_rows_carry_rail() -> Result<()> {
    // -- Setup & Fixtures: a soft break splits the paragraph into two rows.
    let entry = message_entry(MessageRole::User, "first line\nsecond line");

    // -- Exec
    let backend = render(&entry)?;

    // -- Check
    let rows = measured_rows(&entry)?;
    assert!(rows >= 2, "the fixture must render two rows; got {rows}");
    for y in 0..u16::try_from(rows)? {
        assert_user_row(&backend, y)?;
    }
    Ok(())
}

/// WHEN a user message wraps, THE dotted rail SHALL sit at column 0 on the row
/// and on every wrapped continuation row.
#[test]
fn test_renderer_lane_prefix_user_wrapped_rows_carry_rail() -> Result<()> {
    // -- Setup & Fixtures
    let entry = message_entry(MessageRole::User, &long_prose());

    // -- Exec
    let backend = render(&entry)?;

    // -- Check
    let rows = measured_rows(&entry)?;
    assert!(rows >= 2, "the fixture must wrap; got {rows}");
    for y in 0..u16::try_from(rows)? {
        assert_user_row(&backend, y)?;
    }
    Ok(())
}

/// WHEN a tool call's summary wraps, THE continuation rows SHALL keep the blank
/// marker slot — the icon renders on row 0 only.
#[test]
fn test_renderer_lane_prefix_tool_continuation_rows_blank_the_marker() -> Result<()> {
    // -- Setup & Fixtures: a summary far wider than the pane.
    let entry = tool_entry(
        "read",
        &format!("\u{2192} {}", "x".repeat(200)),
        None,
        None,
        ItemStatus::Done,
    );

    // -- Exec
    let backend = render(&entry)?;

    // -- Check
    let rows = measured_rows(&entry)?;
    assert!(rows >= 2, "the summary must wrap; got {rows}");
    assert!(
        row_text(&backend, 0).contains('\u{2699}'),
        "row 0 carries the tool icon"
    );
    for y in 1..u16::try_from(rows)? {
        let row = row_text(&backend, y);
        assert!(
            !row.contains('\u{2699}'),
            "continuation row {y} must blank the marker; got {row:?}"
        );
        assert!(
            row.starts_with("    ") && !row.starts_with("     "),
            "continuation row {y} must start its text at column 4; got {row:?}"
        );
    }
    Ok(())
}

/// WHEN a compaction notice wraps, THE continuation rows SHALL keep the blank
/// marker slot — the tilde renders on row 0 only.
#[test]
fn test_renderer_lane_prefix_compaction_continuation_rows_blank_the_marker() -> Result<()> {
    // -- Setup & Fixtures
    let entry = notice_entry(NoticeKind::Compaction, &"compact ".repeat(30));

    // -- Exec
    let backend = render(&entry)?;

    // -- Check
    let rows = measured_rows(&entry)?;
    assert!(rows >= 2, "the notice must wrap; got {rows}");
    assert!(
        row_text(&backend, 0).contains('~'),
        "row 0 carries the compaction marker"
    );
    for y in 1..u16::try_from(rows)? {
        let row = row_text(&backend, y);
        assert!(
            !row.contains('~'),
            "continuation row {y} must blank the marker; got {row:?}"
        );
        assert!(
            row.starts_with("    "),
            "continuation row {y} must start its text at column 4; got {row:?}"
        );
    }
    Ok(())
}

// endregion: --- Continuation rows

// region:    --- Support

/// Assert one user row carries the dotted rail at column 0 and its text at 2.
fn assert_user_row(backend: &RatatuiBackend, y: u16) -> Result<()> {
    let row = row_text(backend, y);
    assert!(
        row.starts_with("\u{254e} "),
        "row {y} must carry the dotted rail at column 0; got {row:?}"
    );
    assert!(
        row.chars().nth(2) != Some(' '),
        "row {y} must start its text at column 2; got {row:?}"
    );
    assert!(
        !row.contains('\u{258f}'),
        "no user row may carry the legacy rail; got {row:?}"
    );
    Ok(())
}

// endregion: --- Support
