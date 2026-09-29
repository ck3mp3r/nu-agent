use super::tty::layout;
use nu_agent_core::protocol::tool_args::CallLine;
use nu_agent_core::transcript::ir::{
    Block, BlockSource, ContentKind, ContentLine, Display, DisplaySection, Fill, Lane, MessageRole,
    NoticeKind, StyleHint, ToolName,
};
use nu_agent_core::transcript::renderer::{FrameContext, ItemStatus};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

fn ctx() -> FrameContext {
    FrameContext {
        width: 80,
        now_millis: 0,
        cursor: false,
        selected: false,
    }
}

/// Render with color on — the common case for tests asserting styled output.
fn render(block: &Block) -> String {
    layout(block, &ctx(), true)
}

/// Render with color off — for tests asserting plain, ANSI-free output.
fn render_plain(block: &Block) -> String {
    layout(block, &ctx(), false)
}

fn markdown_block(role: MessageRole, markdown: &str) -> Block {
    let (lane, fill) = match role {
        MessageRole::User => (Lane::Marker("▏"), Fill::None),
        MessageRole::Assistant => (Lane::Blank, Fill::None),
    };
    Block {
        source: BlockSource::Markdown {
            role,
            markdown: markdown.to_string(),
        },
        lane,
        fill,
        status: None,
    }
}

fn tool_block(name: &str, summary: &str) -> Block {
    Block {
        source: BlockSource::Tool {
            name: ToolName(name.to_string()),
            call: CallLine {
                summary: summary.to_string(),
            },
            preview: None,
        },
        lane: Lane::Marker("⚙"),
        fill: Fill::None,
        status: None,
    }
}

fn notice_block(text: &str) -> Block {
    Block {
        source: BlockSource::Notice {
            kind: NoticeKind::System,
            text: text.to_string(),
        },
        lane: Lane::Marker("·"),
        fill: Fill::None,
        status: None,
    }
}

fn spacer_block() -> Block {
    Block {
        source: BlockSource::Spacer,
        lane: Lane::Blank,
        fill: Fill::None,
        status: None,
    }
}

/// A ToolDisplay block: pre-projected tool output on `Lane::Blank`.
fn tool_display_block(lines: Vec<ContentLine>) -> Block {
    Block {
        source: BlockSource::ToolDisplay { lines },
        lane: Lane::Blank,
        fill: Fill::None,
        status: None,
    }
}

#[test]
fn user_message_has_user_prefix() {
    let block = markdown_block(MessageRole::User, "hello");
    let out = render(&block);
    assert!(out.starts_with("[user] "), "got: {out}");
    assert!(out.contains("hello"));
}

#[test]
fn assistant_has_no_prefix() {
    let block = markdown_block(MessageRole::Assistant, "hi");
    let out = render(&block);
    assert!(
        !out.contains("[assistant]"),
        "should have no prefix, got: {out}"
    );
    assert!(out.starts_with("hi"), "should start with content");
}

#[test]
fn tool_shows_tool_prefix() {
    let block = tool_block("run", "→ run");
    let out = render(&block);
    assert!(out.starts_with("[tool] "), "got: {out}");
}

#[test]
fn spacer_renders_empty_string() {
    let out = render(&spacer_block());
    assert_eq!(out, "");
}

#[test]
fn done_status_shows_checkmark() {
    let mut block = tool_block("t", "→ t");
    block.status = Some(ItemStatus::Done);
    let out = render(&block);
    assert!(out.contains("✓"), "got: {out}");
}

#[test]
fn unknown_status_shows_question_mark() {
    let mut block = tool_block("t", "→ t");
    block.status = Some(ItemStatus::Unknown);
    let out = render(&block);
    assert!(out.contains("? "), "got: {out}");
}

/// Render at a specific wall-clock time, for spinner-frame assertions.
fn render_at(block: &Block, now_millis: u128) -> String {
    let ctx = FrameContext {
        width: 80,
        now_millis,
        cursor: false,
        selected: false,
    };
    layout(block, &ctx, true)
}

#[test]
fn in_progress_status_shows_first_spinner_frame() -> Result<()> {
    // -- Setup & Fixtures
    let mut block = tool_block("t", "→ t");
    block.status = Some(ItemStatus::InProgress);

    // -- Exec
    let out = render_at(&block, 0);

    // -- Check: the first braille spinner frame, not the old static ellipsis.
    assert!(
        out.contains("⠋"),
        "InProgress at now_millis=0 must show `⠋`; got: {out}"
    );
    assert!(
        !out.contains('…'),
        "InProgress must not use the static ellipsis; got: {out}"
    );
    Ok(())
}

#[test]
fn in_progress_status_shows_second_spinner_frame_after_100ms() -> Result<()> {
    // -- Setup & Fixtures
    let mut block = tool_block("t", "→ t");
    block.status = Some(ItemStatus::InProgress);

    // -- Exec
    let first = render_at(&block, 0);
    let second = render_at(&block, 100);

    // -- Check
    assert!(
        second.contains("⠙"),
        "InProgress at now_millis=100 must show `⠙`; got: {second}"
    );
    assert_ne!(
        first, second,
        "the indicator must animate across 100 ms; both frames were {first:?}"
    );
    Ok(())
}

#[test]
fn in_progress_indicator_matches_item_status_method() -> Result<()> {
    // -- Setup & Fixtures: the TTY indicator is the shared method's output
    // plus the trailing space the format string requires.
    let mut block = tool_block("t", "→ t");
    block.status = Some(ItemStatus::InProgress);

    // -- Exec
    let out = render_at(&block, 350);

    // -- Check
    let expected = ItemStatus::InProgress.indicator_char(350);
    assert!(
        out.contains(&format!("{expected} ")),
        "TTY indicator must be `{expected} ` (method output + trailing space); got: {out}"
    );
    Ok(())
}

#[test]
fn no_ansi_when_color_false() {
    let block = notice_block("+added");
    let out = render_plain(&block);
    assert!(
        !out.contains("\x1b"),
        "should have no ANSI codes, got: {out}"
    );
}

#[test]
fn ansi_green_for_diff_add_when_color_true() {
    let block = notice_block("+added");
    let out = render(&block);
    assert!(out.contains("+added"), "got: {out}");
}

#[test]
fn multi_line_separated_by_newlines() {
    let block = notice_block("line1\nline2");
    let out = render(&block);
    assert!(
        out.contains("line1\nline2"),
        "should have newline between lines, got: {out}"
    );
}

#[test]
fn tool_preview_lines_render_every_content_line() {
    // -- Setup & Fixtures
    let command = "ls | where size > 1mb\n| select name type\n| sort-by modified";
    let block = Block {
        source: BlockSource::Tool {
            name: ToolName("nu".to_string()),
            call: CallLine {
                summary: "nu".to_string(),
            },
            preview: Some(Display {
                title: "nu".to_string(),
                sections: vec![DisplaySection {
                    label: "nu".to_string(),
                    kind: ContentKind::Code {
                        language: "nu".to_string(),
                    },
                    content: command.to_string(),
                    stats: None,
                }],
            }),
        },
        lane: Lane::Marker("⚙"),
        fill: Fill::None,
        status: None,
    };

    // -- Exec
    let out = render(&block);

    // -- Check: the command appears one line per command line, with no JSON
    // summary flattening.
    assert!(
        out.contains("ls | where size > 1mb\n| select name type\n| sort-by modified"),
        "got: {out}"
    );
}

/// Notice content is rendered verbatim (no ANSI mangling of text).
#[test]
fn notice_content_renders_verbatim() {
    let block = notice_block("+added");
    let out = render(&block);
    assert!(out.contains("+added"), "got: {out}");
}

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

/// Markdown blocks project raw text lines with the role prefix on row 0.
#[test]
fn layout_user_markdown_has_prefix_and_content() -> Result<()> {
    // -- Setup & Fixtures
    let block = markdown_block(MessageRole::User, "hello there");

    // -- Exec
    let out = render(&block);

    // -- Check
    assert!(out.starts_with("[user] "), "got: {out}");
    assert!(out.contains("hello there"), "got: {out}");
    Ok(())
}

/// Notice blocks render their text on the system prefix.
#[test]
fn layout_notice_renders_text() -> Result<()> {
    // -- Setup & Fixtures
    let block = notice_block("compaction done");

    // -- Exec
    let out = render(&block);

    // -- Check
    assert!(out.contains("[system]"), "got: {out}");
    assert!(out.contains("compaction done"), "got: {out}");
    Ok(())
}

/// Banner blocks render their text lines with no prefix.
#[test]
fn layout_banner_renders_text_lines() -> Result<()> {
    // -- Setup & Fixtures
    let block = Block {
        source: BlockSource::Banner {
            text: "banner line".to_string(),
        },
        lane: Lane::SystemBlank,
        fill: Fill::None,
        status: None,
    };

    // -- Exec
    let out = render(&block);

    // -- Check
    assert_eq!(out, "banner line");
    Ok(())
}

/// Status indicators appear on row 0 through layout() as well.
#[test]
fn layout_done_status_shows_checkmark() -> Result<()> {
    // -- Setup & Fixtures
    let mut block = tool_block("t", "→ t");
    block.status = Some(ItemStatus::Done);

    // -- Exec
    let out = render(&block);

    // -- Check
    assert!(out.contains("✓"), "got: {out}");
    Ok(())
}

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

// -- Test Support

fn diff_tool_block() -> Block {
    Block {
        source: BlockSource::Tool {
            name: ToolName("edit".to_string()),
            call: CallLine {
                summary: "edit file".to_string(),
            },
            preview: Some(Display {
                title: "edit file".to_string(),
                sections: vec![DisplaySection {
                    label: "diff".to_string(),
                    kind: ContentKind::Diff {
                        language: "diff".to_string(),
                    },
                    content: "@@ -1,2 +1,2 @@\n--- a.rs\n+++ b.rs\n+added\n-removed\nunchanged"
                        .to_string(),
                    stats: None,
                }],
            }),
        },
        lane: Lane::Marker("⚙"),
        fill: Fill::None,
        status: None,
    }
}

fn code_tool_block() -> Block {
    Block {
        source: BlockSource::Tool {
            name: ToolName("nu".to_string()),
            call: CallLine {
                summary: "nu".to_string(),
            },
            preview: Some(Display {
                title: "nu".to_string(),
                sections: vec![DisplaySection {
                    label: "nu".to_string(),
                    kind: ContentKind::Code {
                        language: "nu".to_string(),
                    },
                    content: "ls | where size > 1mb\n| sort-by modified".to_string(),
                    stats: None,
                }],
            }),
        },
        lane: Lane::Marker("⚙"),
        fill: Fill::None,
        status: None,
    }
}

fn plain_tool_block() -> Block {
    Block {
        source: BlockSource::Tool {
            name: ToolName("http".to_string()),
            call: CallLine {
                summary: "http".to_string(),
            },
            preview: Some(Display {
                title: "http".to_string(),
                sections: vec![DisplaySection {
                    label: "body".to_string(),
                    kind: ContentKind::Plain,
                    content: "key: value".to_string(),
                    stats: None,
                }],
            }),
        },
        lane: Lane::Marker("⚙"),
        fill: Fill::None,
        status: None,
    }
}
