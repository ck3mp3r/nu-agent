//! Transcript column alignment: the lane prefix pins every lane to a fixed
//! gutter, and the wrap budget keeps the list area's last two columns clear
//! (task 45b99a84).
//!
//! Coordinates are LIST-LOCAL: 0 is the transcript list area's first column.
//! The tool, compaction, and system lanes render `[cursor slot][marker]` with
//! text at 4; the user lane renders the dotted rail at 0 with text at 2; the
//! assistant lane renders a blank 2-column prefix with text at 2. The banner
//! lane is centered within the pane separately and keeps its own rendering.

use super::support::{frame_ctx, markdown_block};
use crate::rendering::theme::TuiTheme;
use crate::tui_renderer::layout;
use agnt_core::protocol::tool_args::CallLine;
use agnt_core::transcript::ir::*;
use unicode_width::UnicodeWidthStr;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

const WIDTH: usize = 80;

// ========== lane gutters ==========

/// WHEN `layout()` renders a tool, compaction, or system block that wraps, THE
/// marker glyph SHALL sit at list-local 2 with text at list-local 4 on row 0,
/// and every continuation row SHALL keep the blank marker slot with text at
/// list-local 4 — unchanged by the gutter task.
#[test]
fn layout_tool_lanes_keep_marker_at_two_and_text_at_four() -> Result<()> {
    // -- Setup & Fixtures
    let theme = TuiTheme::default();
    let tool = Block {
        source: BlockSource::Tool {
            name: ToolName("tool".to_string()),
            call: CallLine {
                summary: long_prose(),
            },
            preview: None,
        },
        lane: Lane::Marker("\u{2699}"),
        fill: Fill::None,
        status: None,
    };
    let compaction = Block {
        source: BlockSource::Notice {
            kind: NoticeKind::Compaction,
            text: long_prose(),
        },
        lane: Lane::Marker("~"),
        fill: Fill::None,
        status: None,
    };
    let system = Block {
        source: BlockSource::Notice {
            kind: NoticeKind::System,
            text: long_prose(),
        },
        lane: Lane::Marker("\u{00b7}"),
        fill: Fill::None,
        status: None,
    };

    // -- Exec & Check
    for (block, row_zero_prefix) in [
        (&tool, "  \u{2699} "),
        (&compaction, "  ~ "),
        (&system, "  \u{00b7} "),
    ] {
        let lines = layout(block, &frame_ctx(WIDTH), &theme);
        assert!(
            lines.len() >= 2,
            "fixture must wrap at width {WIDTH}; got {}",
            lines.len()
        );
        let row0 = lines.first().ok_or("row 0 must exist")?;
        assert!(
            line_text(row0).starts_with(row_zero_prefix),
            "row 0 of lane {:?} must render the marker at 2 and text at 4; got {:?}",
            block.lane,
            line_text(row0)
        );
        for (idx, line) in lines.iter().enumerate().skip(1) {
            assert!(
                line_text(line).starts_with("    "),
                "continuation row {idx} of lane {:?} must keep the blank marker slot (text at 4); got {:?}",
                block.lane,
                line_text(line)
            );
        }
    }
    Ok(())
}

/// WHEN `layout()` renders a user block that wraps, THE dotted rail `╎` SHALL
/// sit at list-local 0 on row 0 and on every continuation row, THE text SHALL
/// start at list-local 2, and no row SHALL contain the legacy rail.
#[test]
fn layout_user_rows_carry_dotted_rail_at_zero_and_text_at_two() -> Result<()> {
    // -- Setup & Fixtures
    let block = markdown_block(MessageRole::User, &long_prose(), Fill::Full);

    // -- Exec
    let lines = layout(&block, &frame_ctx(WIDTH), &TuiTheme::default());

    // -- Check
    assert!(
        lines.len() >= 2,
        "fixture must wrap at width {WIDTH}; got {}",
        lines.len()
    );
    for (idx, line) in lines.iter().enumerate() {
        let text = line_text(line);
        assert!(
            text.starts_with("\u{254e} "),
            "user row {idx} must carry the dotted rail at 0; got {text:?}"
        );
        assert!(
            text.chars().count() > 2,
            "user row {idx} must start its text at 2; got {text:?}"
        );
        assert!(
            !text.contains('\u{258f}'),
            "user row {idx} must not carry the legacy rail; got {text:?}"
        );
    }
    Ok(())
}

/// WHEN `layout()` renders an assistant block that wraps, THE text SHALL start
/// at list-local 2 (the cog column) on row 0 and on every continuation row.
#[test]
fn layout_assistant_rows_start_text_at_two() -> Result<()> {
    // -- Setup & Fixtures
    let block = markdown_block(MessageRole::Assistant, &long_prose(), Fill::None);

    // -- Exec
    let lines = layout(&block, &frame_ctx(WIDTH), &TuiTheme::default());

    // -- Check
    assert!(
        lines.len() >= 2,
        "fixture must wrap at width {WIDTH}; got {}",
        lines.len()
    );
    for (idx, line) in lines.iter().enumerate() {
        let text = line_text(line);
        assert!(
            text.starts_with("  ") && !text.starts_with("    "),
            "assistant row {idx} must start its text at 2; got {text:?}"
        );
    }
    Ok(())
}

// ========== right margin ==========

/// WHEN any entry lane renders a wrapped block, THE last text column SHALL be
/// at most `WIDTH - 3`: the wrap budget keeps the list area's last two columns
/// clear in every lane.
#[test]
fn layout_every_entry_lane_keeps_the_right_margin_clear() -> Result<()> {
    // -- Setup & Fixtures: one wrapping block per entry lane.
    let theme = TuiTheme::default();
    let user = markdown_block(MessageRole::User, &long_prose(), Fill::Full);
    let assistant = markdown_block(MessageRole::Assistant, &long_prose(), Fill::None);
    let tool = Block {
        source: BlockSource::Tool {
            name: ToolName("tool".to_string()),
            call: CallLine {
                summary: long_prose(),
            },
            preview: None,
        },
        lane: Lane::Marker("\u{2699}"),
        fill: Fill::None,
        status: None,
    };
    let system = Block {
        source: BlockSource::Notice {
            kind: NoticeKind::System,
            text: long_prose(),
        },
        lane: Lane::Marker("\u{00b7}"),
        fill: Fill::None,
        status: None,
    };

    // -- Exec & Check
    for block in [&user, &assistant, &tool, &system] {
        let lines = layout(block, &frame_ctx(WIDTH), &theme);
        assert!(
            lines.len() >= 2,
            "fixture must wrap at width {WIDTH}; got {}",
            lines.len()
        );
        for (idx, line) in lines.iter().enumerate() {
            let width = line_width(line);
            assert!(
                width <= WIDTH - 2,
                "row {idx} of lane {:?} must leave the last two columns clear (max width {}); got width {width}: {:?}",
                block.lane,
                WIDTH - 2,
                line_text(line)
            );
        }
    }
    Ok(())
}

/// WHEN `measure()` and `layout()` process the same wrapped block, THE visual
/// row counts SHALL stay identical for every lane — the lane prefix feeds both
/// budgets from one source.
#[test]
fn measure_matches_layout_rows_for_every_lane() -> Result<()> {
    // -- Setup & Fixtures
    let theme = TuiTheme::default();
    let user = markdown_block(MessageRole::User, &long_prose(), Fill::Full);
    let assistant = markdown_block(MessageRole::Assistant, &long_prose(), Fill::None);

    // -- Exec & Check
    for block in [&user, &assistant] {
        let measured = crate::tui_renderer::measure(block, WIDTH, &theme);
        let laid_out = layout(block, &frame_ctx(WIDTH), &theme).len();
        assert_eq!(
            measured, laid_out,
            "measure() must equal layout().len() for lane {:?}",
            block.lane
        );
    }
    Ok(())
}

// -- Test Support

/// Prose long enough to wrap at [`WIDTH`]: 12 nine-character words.
fn long_prose() -> String {
    (0..12)
        .map(|index| format!("word{index:05}"))
        .collect::<Vec<String>>()
        .join(" ")
}

fn line_text(line: &ratatui::text::Line<'static>) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

fn line_width(line: &ratatui::text::Line<'static>) -> usize {
    line.spans
        .iter()
        .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
        .sum()
}
