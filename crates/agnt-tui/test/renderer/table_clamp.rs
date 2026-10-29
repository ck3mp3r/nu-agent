//! Table clamping in the transcript: the projector's clamp target must be the
//! renderer's wrap budget, or a table that "fits" the canvas gets re-wrapped
//! and its box drawing is shredded.

use super::support::{frame_ctx, markdown_block};
use crate::rendering::theme::TuiTheme;
use crate::state::code_block::content_wrap_width;
use crate::tui_renderer::{lane_prefix_width, layout, measure};
use agnt_core::transcript::ir::{Fill, Lane, MessageRole};
use unicode_width::UnicodeWidthStr;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Natural table width 37 (4 columns, 6 display columns each): wider than the
/// wrap budget of a 40-column canvas (36) but inside the canvas itself. The
/// pre-fix renderer projected at 40, saw the table fit, then wrapped its 37-wide
/// rows at 36 — every border and row line split.
const WIDE_TABLE: &str = "| Name | Status | Owner | Notes |\n\
| --- | --- | --- | --- |\n\
| abcdef | ghijkl | mnopqr | stuvwx |";

fn line_width(line: &ratatui::text::Line<'static>) -> usize {
    line.spans
        .iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum()
}

fn line_text(line: &ratatui::text::Line<'static>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

/// WHEN a Markdown block holds a table whose natural width sits between the
/// wrap budget and the canvas width, THE renderer SHALL clamp it to the wrap
/// budget so no row is re-wrapped and the top border stays unbroken.
#[test]
fn layout_table_clamp_keeps_borders_unbroken_at_wrap_budget() -> Result<()> {
    // -- Setup & Fixtures
    let theme = TuiTheme::default();
    let width = 40;
    let block = markdown_block(MessageRole::Assistant, WIDE_TABLE, Fill::None);
    assert!(
        content_wrap_width(width, false, lane_prefix_width(&Lane::Blank)) < 37,
        "fixture must straddle the wrap budget (budget={})",
        content_wrap_width(width, false, lane_prefix_width(&Lane::Blank))
    );

    // -- Exec
    let lines = layout(&block, &frame_ctx(width), &theme);

    // -- Check: no rendered row exceeds the canvas.
    for line in &lines {
        assert!(
            line_width(line) <= width,
            "row exceeds canvas width {width}: {:?} (width={})",
            line_text(line),
            line_width(line)
        );
    }

    // -- Check: the top border is one unbroken row.
    let top = lines
        .iter()
        .map(line_text)
        .find(|text| text.contains('╭'))
        .ok_or("table must render a top border")?;
    assert!(
        top.contains('╮'),
        "top border must stay on one row: {top:?}"
    );

    // -- Check: the bottom border is one unbroken row too.
    let bottom = lines
        .iter()
        .map(line_text)
        .find(|text| text.contains('╰'))
        .ok_or("table must render a bottom border")?;
    assert!(
        bottom.contains('╯'),
        "bottom border must stay on one row: {bottom:?}"
    );
    Ok(())
}

/// WHEN a table's natural width exceeds the wrap budget, THE renderer SHALL keep
/// every column — cell contents wrap inside their column — and emit no wrapped
/// border fragment: every box row stays one transcript row.
#[test]
fn layout_table_clamp_keeps_all_columns_within_canvas() -> Result<()> {
    // -- Setup & Fixtures
    let theme = TuiTheme::default();
    let width = 40;
    let block = markdown_block(MessageRole::Assistant, WIDE_TABLE, Fill::None);

    // -- Exec
    let lines = layout(&block, &frame_ctx(width), &theme);
    let texts: Vec<String> = lines.iter().map(line_text).collect();

    // -- Check: every column survives; nothing is dropped or announced.
    let joined = texts.join("\n");
    for label in ["Name", "Status", "Owner", "Notes"] {
        assert!(
            joined.contains(label),
            "column header {label} must render; got:\n{joined}"
        );
    }
    assert!(
        !joined.contains("hidden"),
        "no column is dropped, so no hidden-columns marker; got:\n{joined}"
    );

    // -- Check: every row fits the canvas, so the word wrapper never splits a box
    // row into fragments.
    for line in &lines {
        assert!(
            line_width(line) <= width,
            "row exceeds canvas width {width}: {:?} (width={})",
            line_text(line),
            line_width(line)
        );
    }

    // -- Check: each box row carries the full border set — 4 columns need 5 `│`.
    // The narrow first column wraps its `abcdef` cell into two physical rows.
    let box_rows: Vec<&String> = texts.iter().filter(|text| text.contains('│')).collect();
    assert_eq!(
        box_rows.len(),
        3,
        "header + 2 wrapped data rows expected; got:\n{joined}"
    );
    for row in box_rows {
        assert_eq!(
            row.matches('│').count(),
            5,
            "every physical row keeps all 5 cell separators: {row:?}"
        );
    }
    Ok(())
}

/// WHEN `measure()` and `layout()` process the same table block at the same
/// width, THE measured row count SHALL equal the laid-out row count — both
/// project at the wrap budget, or the height index desyncs from the render.
#[test]
fn measure_table_clamp_matches_layout_row_count() {
    // -- Setup & Fixtures
    let theme = TuiTheme::default();
    let width = 40;
    let block = markdown_block(MessageRole::Assistant, WIDE_TABLE, Fill::None);

    // -- Exec
    let measured = measure(&block, width, &theme);
    let laid_out = layout(&block, &frame_ctx(width), &theme).len();

    // -- Check
    assert_eq!(
        measured, laid_out,
        "measure() must equal layout().len() for a clamped table block"
    );
}
