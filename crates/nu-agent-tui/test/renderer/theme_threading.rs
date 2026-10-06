use super::support::{
    content_spans, diff_tint_block, frame_ctx, markdown_block, rows_containing, span_containing,
    tool_code_block,
};
use crate::rendering::theme::TuiTheme;
use crate::tui_renderer::{layout, measure};
use nu_agent_core::transcript::ir::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ========== active theme threading (task 7bd4c724) ==========

/// WHEN `layout()` receives a non-default theme, THE syntax/diff colours SHALL
/// resolve from that theme, not from `TuiTheme::default()`.
#[test]
fn layout_uses_caller_provided_theme_for_diff_tint() -> Result<()> {
    // -- Setup & Fixtures
    let mocha = TuiTheme::default();
    let latte = TuiTheme::catppuccin_latte();
    assert_ne!(
        mocha.diff_add_bg, latte.diff_add_bg,
        "fixture requires the two themes to differ on the add tint"
    );
    let block = diff_tint_block("--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\n");

    // -- Exec
    let lines = layout(&block, &frame_ctx(80), &latte);

    // -- Check: the added row carries the Latte tint, not the Mocha default.
    let added = rows_containing(&lines, "new");
    assert_eq!(added.len(), 1, "exactly one row must carry the added body");
    for span in content_spans(added[0]) {
        assert_eq!(
            span.style.bg,
            Some(latte.diff_add_bg),
            "added line must carry the caller-provided theme's tint; span '{}' has {:?}",
            span.content,
            span.style.bg
        );
    }
    Ok(())
}

/// WHEN `layout()` receives a non-default theme, THE lane row styles SHALL
/// derive from that theme, not from `TuiTheme::default()`.
#[test]
fn layout_uses_caller_provided_theme_for_lane_row_style() -> Result<()> {
    // -- Setup & Fixtures
    let mocha = TuiTheme::default();
    let latte = TuiTheme::catppuccin_latte();
    assert_ne!(
        mocha.row_user_bg, latte.row_user_bg,
        "fixture requires the two themes to differ on the user row background"
    );
    let block = markdown_block(MessageRole::User, "hi", Fill::Full);

    // -- Exec
    let lines = layout(&block, &frame_ctx(80), &latte);

    // -- Check
    let span = span_containing(&lines, "hi").ok_or("user content span must exist")?;
    assert_eq!(
        span.style.bg,
        Some(latte.row_user_bg),
        "user row must carry the caller-provided theme's row background"
    );
    Ok(())
}

/// WHEN `measure()` receives a non-default theme, THE row count SHALL equal
/// the count `layout()` produces for the same theme.
#[test]
fn measure_uses_caller_provided_theme() -> Result<()> {
    // -- Setup & Fixtures
    let theme = TuiTheme::catppuccin_frappe();
    let block = tool_code_block("ls | where size > 1mb");
    let ctx = frame_ctx(80);

    // -- Exec
    let measured = measure(&block, 80, &theme);
    let laid_out = layout(&block, &ctx, &theme).len();

    // -- Check
    assert_eq!(
        measured, laid_out,
        "measure() must match layout() row count for the caller-provided theme"
    );
    Ok(())
}
