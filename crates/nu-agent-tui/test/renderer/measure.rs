use super::support::{frame_ctx, markdown_block, spacer_block, tool_code_block, tool_diff_block};
use crate::rendering::theme::TuiTheme;
use crate::tui_renderer::{layout, measure};
use nu_agent_core::protocol::tool_args::CallLine;
use nu_agent_core::transcript::ir::*;
use nu_agent_core::transcript::renderer::ItemStatus;

// ========== measure() tests (task 53279c82) ==========

#[test]
fn measure_markdown_matches_layout_row_count() {
    // -- Setup & Fixtures
    let block = markdown_block(MessageRole::User, "short line", Fill::None);
    let ctx = frame_ctx(80);
    let theme = TuiTheme::default();

    // -- Exec
    let measured = measure(&block, 80, &theme);
    let laid_out = layout(&block, &ctx, &theme).len();

    // -- Check
    assert_eq!(
        measured, laid_out,
        "measure() must equal layout().len() for a markdown block"
    );
}

#[test]
fn measure_wrapping_prose_matches_layout_row_count() {
    // -- Setup & Fixtures
    let block = markdown_block(
        MessageRole::User,
        "aaaaaaaaaa bbbbbbbbbb cccccccccc dddddddddd",
        Fill::Full,
    );
    let ctx = frame_ctx(24);
    let theme = TuiTheme::default();

    // -- Exec
    let measured = measure(&block, 24, &theme);
    let laid_out = layout(&block, &ctx, &theme).len();

    // -- Check
    assert!(laid_out >= 2, "prose must wrap at width 24; got {laid_out}");
    assert_eq!(
        measured, laid_out,
        "measure() must match layout() row count for wrapping prose"
    );
}

/// WHEN `measure()` and `layout()` run on a `Fill::Code` block, THE measured
/// row count SHALL equal the laid-out row count, including the two margin
/// rows. Pins the renderer contract for any `Fill::Code` block; in production
/// that block is a `ToolDisplay` preview.
#[test]
fn measure_tool_code_preview_includes_two_margin_rows() {
    // -- Setup & Fixtures
    let block = tool_code_block("ls | where size > 1mb");
    let ctx = frame_ctx(80);
    let theme = TuiTheme::default();

    // -- Exec
    let measured = measure(&block, 80, &theme);
    let laid_out = layout(&block, &ctx, &theme).len();

    // -- Check: Fill::Code must contribute the 2 margin rows above/below.
    assert_eq!(
        measured, laid_out,
        "measure() must equal layout() row count for Tool+Code preview (incl. margins)"
    );
    assert!(
        measured >= 4,
        "Fill::Code measure must include 2 margin rows + content; got {measured}"
    );
}

#[test]
fn measure_tool_diff_matches_layout_row_count() {
    // -- Setup & Fixtures
    let block = tool_diff_block("--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\n");
    let ctx = frame_ctx(80);
    let theme = TuiTheme::default();

    // -- Exec
    let measured = measure(&block, 80, &theme);
    let laid_out = layout(&block, &ctx, &theme).len();

    // -- Check
    assert_eq!(
        measured, laid_out,
        "measure() must equal layout() row count for a Tool+Diff block"
    );
}

#[test]
fn measure_spacer_matches_layout_row_count() {
    // -- Setup & Fixtures
    let block = spacer_block();
    let ctx = frame_ctx(40);
    let theme = TuiTheme::default();

    // -- Exec
    let measured = measure(&block, 40, &theme);
    let laid_out = layout(&block, &ctx, &theme).len();

    // -- Check
    assert_eq!(laid_out, 1, "spacer lays out one row");
    assert_eq!(
        measured, laid_out,
        "measure() must match layout() row count for a spacer"
    );
}

#[test]
fn measure_with_status_indicator_matches_layout_row_count() {
    // -- Setup & Fixtures: the indicator occupies 2 columns on row 0, so the
    // wrap budget shrinks and long row-0 content may wrap into more rows.
    let long_summary = format!("\u{2192} {}", "x".repeat(120));
    let block = Block {
        source: BlockSource::Tool {
            name: ToolName("tool".to_string()),
            call: CallLine {
                summary: long_summary,
            },
            preview: None,
        },
        lane: Lane::Marker("⚙"),
        fill: Fill::None,
        status: Some(ItemStatus::InProgress),
    };
    let ctx = frame_ctx(80);
    let theme = TuiTheme::default();

    // -- Exec
    let measured = measure(&block, 80, &theme);
    let laid_out = layout(&block, &ctx, &theme).len();

    // -- Check
    assert_eq!(
        measured, laid_out,
        "measure() must match layout() when a status indicator shrinks the row-0 budget"
    );
}
