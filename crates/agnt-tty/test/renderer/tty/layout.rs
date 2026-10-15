//! `layout()` purity and tool layout tests for the TTY renderer.

use super::*;

// ── layout() ─────────────────────────────────────────────────────────────────

/// Diff preview lines are styled through `annotate_diff_hint` — add green,
/// remove red, hunk bold, file-meta dim — with no inline `starts_with` checks
/// in the TTY crate.
#[test]
fn layout_tool_diff_preview_annotates_via_annotate_diff_hint() -> Result<()> {
    // -- Setup & Fixtures
    let block = diff_tool_block();

    // -- Exec
    let out = render(&block);

    // -- Check: green for `+`, red for `-`, bold for hunk, dim for file meta.
    assert!(out.contains("\x1b[32m+added\x1b[0m"), "got: {out}");
    assert!(out.contains("\x1b[31m-removed\x1b[0m"), "got: {out}");
    assert!(out.contains("\x1b[1m@@ -1,2 +1,2 @@\x1b[0m"), "got: {out}");
    assert!(out.contains("\x1b[2m--- a.rs\x1b[0m"), "got: {out}");
    assert!(out.contains("\x1b[2m+++ b.rs\x1b[0m"), "got: {out}");
    assert!(out.contains("unchanged"), "got: {out}");
    Ok(())
}

/// `layout` is a pure function of (Block, FrameContext, use_color): the same
/// inputs produce the same output, independent of renderer instance state.
#[test]
fn layout_is_pure_same_inputs_same_output() -> Result<()> {
    // -- Setup & Fixtures
    let block = tool_block("run", "→ run");

    // -- Exec
    let first = render(&block);
    let second = render(&block);

    // -- Check
    assert_eq!(first, second);
    Ok(())
}

/// Spacer blocks produce an empty string — no newline, no prefix.
#[test]
fn layout_spacer_returns_empty_string() -> Result<()> {
    // -- Setup & Fixtures
    let block = spacer_block();

    // -- Exec
    let out = render(&block);

    // -- Check
    assert_eq!(out, "");
    Ok(())
}

/// Tool blocks with no preview render the tool name then the call summary on
/// one line. The raw `result` field is not part of BlockSource::Tool and never
/// renders.
#[test]
fn layout_tool_no_preview_renders_name_and_summary() -> Result<()> {
    // -- Setup & Fixtures
    let block = tool_block("read", "→ run thing");

    // -- Exec
    let out = render(&block);

    // -- Check: exactly one line, name first, then the summary.
    assert_eq!(out, "[tool] read → run thing");
    Ok(())
}

/// The tool name renders as a distinct element, not merely embedded in the
/// summary: a tool named `edit` with summary `→ foo.rs` renders `edit`
/// separately from the summary text.
#[test]
fn layout_tool_name_renders_as_distinct_element() -> Result<()> {
    // -- Setup & Fixtures
    let block = tool_block("edit", "→ foo.rs");

    // -- Exec
    let out = render(&block);

    // -- Check
    assert!(out.contains("edit"), "tool name must render, got: {out}");
    assert!(
        out.contains("edit → foo.rs"),
        "tool name must be a separate element before the summary, got: {out}"
    );
    Ok(())
}

/// An empty call summary renders the tool name alone, with no trailing space:
/// the nu tool's command lives in the preview block, not on the call line.
#[test]
fn layout_tool_empty_summary_renders_name_without_trailing_space() -> Result<()> {
    // -- Setup & Fixtures
    let block = tool_block("nu", "");

    // -- Exec
    let out = render_plain(&block);

    // -- Check
    assert_eq!(out, "[tool] nu");
    Ok(())
}

/// A nameless tool block (the legacy `push_transcript_line` path) renders the
/// summary alone, with no leading blank.
#[test]
fn layout_nameless_tool_renders_summary_without_name() -> Result<()> {
    // -- Setup & Fixtures
    let block = tool_block("", "→ foo.rs");

    // -- Exec
    let out = render(&block);

    // -- Check
    assert_eq!(out, "[tool] → foo.rs");
    Ok(())
}

/// Diff preview coloring is gated by `use_color`.
#[test]
fn layout_diff_preview_annotates_add_and_remove() -> Result<()> {
    // -- Setup & Fixtures
    let block = diff_tool_block();

    // -- Exec
    let out = render(&block);

    // -- Check
    assert!(out.contains("\x1b[32m+added\x1b[0m"), "got: {out}");
    assert!(out.contains("\x1b[31m-removed\x1b[0m"), "got: {out}");
    Ok(())
}

/// Code preview sections render their content lines verbatim (TTY renders
/// plain text; syntax highlighting is the TUI's job).
#[test]
fn layout_tool_code_section_renders_lines() -> Result<()> {
    // -- Setup & Fixtures
    let block = code_tool_block();

    // -- Exec
    let out = render(&block);

    // -- Check
    assert!(
        out.contains("ls | where size > 1mb\n| sort-by modified"),
        "got: {out}"
    );
    Ok(())
}

/// Plain preview sections render their content verbatim.
#[test]
fn layout_tool_plain_section_renders_content() -> Result<()> {
    // -- Setup & Fixtures
    let block = plain_tool_block();

    // -- Exec
    let out = render(&block);

    // -- Check
    assert!(out.contains("key: value"), "got: {out}");
    Ok(())
}
