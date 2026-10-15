//! ToolDisplay regression tests for the TTY renderer.

use super::*;

// ── TTY regressions (task 6a4c7a6b) ─────────────────────────────────────────

/// ToolDisplay blocks emit their pre-projected lines verbatim, one per row,
/// with per-span ANSI styling when `use_color` is true.
#[test]
fn layout_tool_display_emits_preprojected_lines() -> Result<()> {
    // -- Setup & Fixtures
    let block = tool_display_block(vec![
        ContentLine::single("alpha".to_string(), StyleHint::Normal),
        ContentLine::single("beta".to_string(), StyleHint::DiffAdd),
    ]);

    // -- Exec
    let out = render(&block);

    // -- Check: Normal span stays unstyled; DiffAdd span gets green.
    assert!(out.contains("alpha\n\x1b[32mbeta\x1b[0m"), "got: {out}");
    Ok(())
}

/// WHEN `layout()` renders a `BlockSource::ToolDisplay` block with
/// `use_color=false`, THE output SHALL NOT contain ANSI escape codes.
#[test]
fn layout_tool_display_use_color_false_emits_no_ansi() -> Result<()> {
    // -- Setup & Fixtures
    let block = tool_display_block(vec![
        ContentLine::single("alpha".to_string(), StyleHint::Normal),
        ContentLine::single("beta".to_string(), StyleHint::DiffAdd),
    ]);

    // -- Exec
    let out = render_plain(&block);

    // -- Check
    assert!(
        !out.contains("\x1b"),
        "use_color=false must emit no ANSI codes, got: {out}"
    );
    assert!(
        out.contains("beta"),
        "content must still render, got: {out}"
    );
    Ok(())
}

/// WHEN `layout()` renders a `BlockSource::ToolDisplay` block with
/// `use_color=true`, THE output SHALL contain ANSI escape codes for styled
/// spans.
#[test]
fn layout_tool_display_use_color_true_emits_ansi() -> Result<()> {
    // -- Setup & Fixtures
    let block = tool_display_block(vec![ContentLine::single(
        "beta".to_string(),
        StyleHint::DiffAdd,
    )]);

    // -- Exec
    let out = render(&block);

    // -- Check
    assert!(
        out.contains("\x1b[32m"),
        "use_color=true must emit ANSI codes, got: {out}"
    );
    Ok(())
}

/// WHEN `layout()` renders a `BlockSource::ToolDisplay` block, THE output
/// SHALL start with `"  "` (the 2-space indent prefix).
#[test]
fn layout_tool_display_starts_with_two_space_indent() -> Result<()> {
    // -- Setup & Fixtures
    let block = tool_display_block(vec![ContentLine::single(
        "alpha".to_string(),
        StyleHint::Normal,
    )]);

    // -- Exec
    let out = render_plain(&block);

    // -- Check
    assert!(
        out.starts_with("  "),
        "ToolDisplay must keep the 2-space indent prefix, got: {out:?}"
    );
    assert!(out.contains("alpha"), "got: {out:?}");
    Ok(())
}

/// WHEN `layout()` renders a `BlockSource::Markdown` block with `Lane::Blank`,
/// THE output SHALL NOT start with `"  "`.
#[test]
fn layout_blank_lane_markdown_has_no_indent() -> Result<()> {
    // -- Setup & Fixtures
    let block = markdown_block(MessageRole::Assistant, "assistant prose");
    assert_eq!(block.lane, Lane::Blank, "fixture must use Lane::Blank");

    // -- Exec
    let out = render_plain(&block);

    // -- Check
    assert!(
        !out.starts_with("  "),
        "assistant markdown must not be indented, got: {out:?}"
    );
    assert!(out.starts_with("assistant prose"), "got: {out:?}");
    Ok(())
}

/// WHEN `layout()` renders a `BlockSource::Markdown` block containing blank
/// lines, THE output SHALL NOT contain empty lines (blank lines filtered).
#[test]
fn layout_markdown_filters_blank_lines() -> Result<()> {
    // -- Setup & Fixtures
    let block = markdown_block(MessageRole::Assistant, "first\n\n\nsecond\n   \nthird");

    // -- Exec
    let out = render_plain(&block);

    // -- Check: blank lines (including whitespace-only) are dropped.
    assert_eq!(
        out, "first\nsecond\nthird",
        "blank lines must be filtered, got: {out:?}"
    );
    Ok(())
}
