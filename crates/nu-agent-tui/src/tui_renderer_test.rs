use crate::rendering::theme::{TuiTheme, hint_to_style};
use crate::tui_renderer::{lane_prefix_width, layout, measure};
use nu_agent_core::protocol::tool_args::CallLine;
use nu_agent_core::transcript::ir::*;
use nu_agent_core::transcript::items::{Banner, Message};
use nu_agent_core::transcript::renderer::{FrameContext, ItemStatus, Renderable};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ========== layout() tests (task 6a7a3916) ==========

fn frame_ctx(width: usize) -> FrameContext {
    FrameContext {
        width,
        now_millis: 0,
        cursor: false,
        selected: false,
    }
}

fn markdown_block(role: MessageRole, markdown: &str, fill: Fill) -> Block {
    let msg = Message {
        role,
        markdown: markdown.to_string(),
    };
    Block {
        source: msg.source(),
        lane: msg.lane(),
        fill,
        status: None,
    }
}

#[test]
fn layout_markdown_fill_none_has_no_background() {
    // -- Setup & Fixtures
    let block = markdown_block(MessageRole::Assistant, "hello", Fill::None);

    // -- Exec
    let lines = layout(&block, &frame_ctx(80));

    // -- Check
    assert!(!lines.is_empty(), "must produce lines");
    let all_text = lines_text(&lines);
    assert!(
        all_text.contains("hello"),
        "must contain content: {all_text:?}"
    );
    for span in lines.iter().flat_map(|l| &l.spans) {
        assert_eq!(
            span.style.bg, None,
            "Fill::None must produce no background fill; span '{}' has {:?}",
            span.content, span.style.bg
        );
    }
}

#[test]
fn layout_markdown_fill_full_sets_user_bg_on_all_rows() {
    // -- Setup & Fixtures
    let long_text = "aaaaaaaaaa bbbbbbbbbb cccccccccc dddddddddd"; // wraps at width 24
    let block = markdown_block(MessageRole::User, long_text, Fill::Full);

    // -- Exec
    let lines = layout(&block, &frame_ctx(24));

    // -- Check
    assert!(
        lines.len() >= 2,
        "prose must wrap at width 24; got {}",
        lines.len()
    );
    let bg = TuiTheme::default().row_user_bg;
    for (row_idx, span) in lines.iter().flat_map(|l| &l.spans).enumerate() {
        assert_eq!(
            span.style.bg,
            Some(bg),
            "Fill::Full row {row_idx} span '{}' must have user bg, got {:?}",
            span.content,
            span.style.bg
        );
    }
}

#[test]
fn layout_tool_code_preview_inserts_margins_with_surface0_bg() -> Result<()> {
    // -- Setup & Fixtures
    let preview = Display {
        title: "nu".to_string(),
        sections: vec![DisplaySection {
            label: "nu".to_string(),
            kind: ContentKind::Code {
                language: "nu".to_string(),
            },
            content: "ls | select name".to_string(),
            stats: None,
        }],
    };
    let block = Block {
        source: BlockSource::Tool {
            name: ToolName("tool".to_string()),
            call: CallLine {
                summary: "ls | select name".to_string(),
            },
            preview: Some(preview),
        },
        lane: Lane::Marker("⚙"),
        fill: Fill::Code,
        status: None,
    };

    // -- Exec
    let lines = layout(&block, &frame_ctx(80));

    // -- Check
    // Fill::Code inserts a top margin, the content rows, then a bottom margin.
    assert!(
        lines.len() >= 4,
        "Fill::Code must insert margin rows above and below; got {} rows",
        lines.len()
    );
    let surface0 = TuiTheme::default().surface0;
    let text = lines_text(&lines);
    assert!(
        text.contains("ls | select name"),
        "must contain command; got {text:?}"
    );

    let last = lines.len() - 1;
    // Top margin (first row) and bottom margin (last row) MUST carry the
    // surface0 fill on a real span — a bare `Line::from("")` has no span and
    // renders unfilled (task 69bd5698, bug 1).
    for (label, idx) in [("top", 0usize), ("bottom", last)] {
        let margin = lines.get(idx).ok_or("margin row must exist")?;
        assert!(
            !margin.spans.is_empty(),
            "{label} margin row must carry a span so its bg renders; got {margin:?}"
        );
        assert!(
            margin.spans.iter().any(|s| s.style.bg == Some(surface0)),
            "{label} margin row must carry a surface0 span; got {margin:?}"
        );
    }

    // The first content row (call line + status) MUST stay untinted.
    let content_row0 = lines.get(1).ok_or("first content row must exist")?;
    for span in &content_row0.spans {
        assert_ne!(
            span.style.bg,
            Some(surface0),
            "first content row (call line) must stay untinted, got {:?}",
            span.style.bg
        );
    }

    // Every inner content row (after row 0, before the bottom margin) MUST
    // carry surface0.
    for (idx, line) in lines.iter().enumerate().take(last).skip(2) {
        for span in &line.spans {
            assert_eq!(
                span.style.bg,
                Some(surface0),
                "Fill::Code content line {idx} span '{}' must carry surface0 bg, got {:?}; line: {line:?}",
                span.content,
                span.style.bg
            );
        }
    }
    Ok(())
}

// ========== Fill::Code margin rows and row-0 tint (task 69bd5698) =========

/// A `Fill::Code` block with a status indicator: row 0 carries the call line
/// and the done glyph, on the tool lane (not tinted).
///
/// This pins the RENDERER contract for any `Fill::Code` block — margins above
/// and below, row 0 untinted at span level. In production the `Fill::Code`
/// producer is a `ToolDisplay` preview block, never a Tool block: the Tool
/// block keeps `Fill::None` so its call line gets neither the code surface nor
/// a margin row (task 4b70df9b).
fn fill_code_tool_block_with_status() -> Block {
    let preview = Display {
        title: "nu".to_string(),
        sections: vec![DisplaySection {
            label: "nu".to_string(),
            kind: ContentKind::Code {
                language: "nu".to_string(),
            },
            content: "ls".to_string(),
            stats: None,
        }],
    };
    Block {
        source: BlockSource::Tool {
            name: ToolName("nu".to_string()),
            call: CallLine {
                summary: "→ ls".to_string(),
            },
            preview: Some(preview),
        },
        lane: Lane::Marker("⚙"),
        fill: Fill::Code,
        status: Some(ItemStatus::Done),
    }
}

/// WHEN `layout()` renders a `Fill::Code` block, THE first row (top margin)
/// SHALL have a span with bg surface0. Pins the renderer contract for any
/// `Fill::Code` block; in production that block is a `ToolDisplay` preview.
#[test]
fn layout_fill_code_top_margin_has_surface0() -> Result<()> {
    // -- Setup & Fixtures
    let block = fill_code_tool_block_with_status();
    let surface0 = TuiTheme::default().surface0;

    // -- Exec
    let lines = layout(&block, &frame_ctx(80));

    // -- Check
    let top = lines.first().ok_or("top margin row must exist")?;
    assert!(
        top.spans.iter().any(|s| s.style.bg == Some(surface0)),
        "top margin row must carry a surface0 span; got {top:?}"
    );
    Ok(())
}

/// WHEN `layout()` renders a `Fill::Code` block, THE last row (bottom margin)
/// SHALL have a span with bg surface0. Pins the renderer contract for any
/// `Fill::Code` block; in production that block is a `ToolDisplay` preview.
#[test]
fn layout_fill_code_bottom_margin_has_surface0() -> Result<()> {
    // -- Setup & Fixtures
    let block = fill_code_tool_block_with_status();
    let surface0 = TuiTheme::default().surface0;

    // -- Exec
    let lines = layout(&block, &frame_ctx(80));

    // -- Check
    let bottom = lines.last().ok_or("bottom margin row must exist")?;
    assert!(
        bottom.spans.iter().any(|s| s.style.bg == Some(surface0)),
        "bottom margin row must carry a surface0 span; got {bottom:?}"
    );
    Ok(())
}

/// WHEN `layout()` renders a `Fill::Code` block with
/// `status: Some(ItemStatus::Done)`, THE first content row (row 0 after the
/// top margin, carrying ⚙ and ✓) SHALL NOT have bg surface0 — it SHALL carry
/// the lane's row style. Pins the renderer contract for any `Fill::Code`
/// block; in production that block is a `ToolDisplay` preview, so the row-0
/// exemption is a renderer-level guarantee rather than a production path.
#[test]
fn layout_fill_code_row_zero_is_untinted() -> Result<()> {
    // -- Setup & Fixtures
    let block = fill_code_tool_block_with_status();
    let theme = TuiTheme::default();

    // -- Exec
    let lines = layout(&block, &frame_ctx(80));

    // -- Check: index 0 is the top margin, index 1 is the first content row.
    let row0 = lines.get(1).ok_or("first content row must exist")?;
    let text: String = row0.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(
        text.contains('✓'),
        "first content row must carry the status glyph; got {text:?}"
    );
    for span in &row0.spans {
        assert_ne!(
            span.style.bg,
            Some(theme.surface0),
            "row 0 (call line + status) must not be tinted; got {:?}",
            span.style.bg
        );
    }
    // It carries the lane row style instead (the lane style wins on row 0).
    let expected_bg = theme.row_tool.bg;
    assert_eq!(
        row0.spans
            .iter()
            .filter(|s| s.style.bg == expected_bg)
            .count(),
        row0.spans.len(),
        "row 0 must carry the lane row style on every span; got {row0:?}"
    );
    Ok(())
}

/// WHEN `layout()` renders a `Fill::Code` block, THE content rows (after
/// row 0, before the bottom margin) SHALL have bg surface0. Pins the renderer
/// contract for any `Fill::Code` block; in production that block is a
/// `ToolDisplay` preview.
#[test]
fn layout_fill_code_inner_content_rows_have_surface0() -> Result<()> {
    // -- Setup & Fixtures
    let block = fill_code_tool_block_with_status();
    let surface0 = TuiTheme::default().surface0;

    // -- Exec
    let lines = layout(&block, &frame_ctx(80));

    // -- Check: rows 2..last are inner content rows (row 1 is row 0).
    let last = lines.len().saturating_sub(1);
    let mut checked = 0usize;
    for (idx, line) in lines.iter().enumerate().take(last).skip(2) {
        checked += 1;
        for span in &line.spans {
            assert_eq!(
                span.style.bg,
                Some(surface0),
                "inner content row {idx} span '{}' must carry surface0, got {:?}",
                span.content,
                span.style.bg
            );
        }
    }
    assert!(
        checked > 0,
        "fixture must produce at least one inner content row"
    );
    Ok(())
}

#[test]
fn layout_tool_preview_diff_renders_diff_lines() {
    // -- Setup & Fixtures
    let preview = Display {
        title: "edit a.rs".to_string(),
        sections: vec![DisplaySection {
            label: "a.rs".to_string(),
            kind: ContentKind::Diff {
                language: "diff".to_string(),
            },
            content: "--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\n".to_string(),
            stats: None,
        }],
    };
    let block = Block {
        source: BlockSource::Tool {
            name: ToolName("tool".to_string()),
            call: CallLine {
                summary: "→ a.rs (diff)".to_string(),
            },
            preview: Some(preview),
        },
        lane: Lane::Marker("⚙"),
        fill: Fill::None,
        status: None,
    };

    // -- Exec
    let lines = layout(&block, &frame_ctx(80));

    // -- Check
    let text = lines_text(&lines);
    assert!(
        text.contains("→ a.rs (diff)"),
        "row 0 must carry the call summary; got {text:?}"
    );
    assert!(
        text.contains("+new") && text.contains("-old"),
        "diff content must be projected; got {text:?}"
    );
}

#[test]
fn layout_tool_without_preview_renders_call_summary_only() {
    // -- Setup & Fixtures
    let block = Block {
        source: BlockSource::Tool {
            name: ToolName("tool".to_string()),
            call: CallLine {
                summary: "→ {\"path\":\"a.rs\"}".to_string(),
            },
            preview: None,
        },
        lane: Lane::Marker("⚙"),
        fill: Fill::None,
        status: None,
    };

    // -- Exec
    let lines = layout(&block, &frame_ctx(80));

    // -- Check
    let text = lines_text(&lines);
    assert!(
        text.contains("→ {\"path\":\"a.rs\"}"),
        "call summary must be on the rows; got {text:?}"
    );
}

// ========== tool call line spans (task 6a10ca2b) ==========

/// The tool call row carries the tool name in emphasis followed by the
/// summary in muted — the pre-Block model's styled call line. Two content
/// spans with different styles, after the 2-span lane prefix and with no
/// status indicator.
#[test]
fn layout_tool_call_line_has_emphasis_name_and_muted_summary() -> Result<()> {
    // -- Setup & Fixtures
    let theme = TuiTheme::default();
    let block = Block {
        source: BlockSource::Tool {
            name: ToolName("edit".to_string()),
            call: CallLine {
                summary: "→ foo.rs".to_string(),
            },
            preview: None,
        },
        lane: Lane::Marker("⚙"),
        fill: Fill::None,
        status: None,
    };

    // -- Exec
    let lines = layout(&block, &frame_ctx(80));

    // -- Check
    let row0 = lines.first().ok_or("row 0 must exist")?;
    // Spans 0 and 1 are the lane prefix (cursor slot + marker); the call line
    // spans follow, with no status indicator on this block.
    let content_spans = row0.spans.get(2..).ok_or("call line spans must exist")?;
    assert!(
        content_spans.len() >= 2,
        "call line must have at least 2 spans, got {content_spans:?}"
    );
    assert_eq!(
        content_spans[0].content.as_ref(),
        "edit",
        "first call-line span must be the tool name"
    );
    assert_eq!(
        content_spans[0].style,
        hint_to_style(&StyleHint::Emphasis, theme.role_tool, &theme),
        "tool name must use the Emphasis style"
    );
    let summary_text: String = content_spans[1..]
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    assert_eq!(summary_text.trim(), "→ foo.rs");
    assert_eq!(
        content_spans[1].style,
        hint_to_style(&StyleHint::Muted, theme.role_tool, &theme),
        "summary must use the Muted style"
    );
    assert_ne!(
        content_spans[0].style, content_spans[1].style,
        "name and summary spans must have different styles"
    );
    Ok(())
}

/// A nameless tool block renders the summary alone — no empty emphasis span.
#[test]
fn layout_nameless_tool_call_line_renders_summary_span_only() -> Result<()> {
    // -- Setup & Fixtures
    let block = Block {
        source: BlockSource::Tool {
            name: ToolName(String::new()),
            call: CallLine {
                summary: "→ foo.rs".to_string(),
            },
            preview: None,
        },
        lane: Lane::Marker("⚙"),
        fill: Fill::None,
        status: None,
    };

    // -- Exec
    let lines = layout(&block, &frame_ctx(80));

    // -- Check
    let row0 = lines.first().ok_or("row 0 must exist")?;
    let content_spans = row0.spans.get(2..).ok_or("call line spans must exist")?;
    assert_eq!(
        content_spans.len(),
        1,
        "nameless tool must render one summary span, got {content_spans:?}"
    );
    assert_eq!(content_spans[0].content.as_ref(), "→ foo.rs");
    Ok(())
}

#[test]
fn layout_notice_renders_single_meta_line() {
    // -- Setup & Fixtures
    let block = Block {
        source: BlockSource::Notice {
            kind: NoticeKind::Compaction,
            text: "compacted 5 blocks".to_string(),
        },
        lane: Lane::Marker("~"),
        fill: Fill::None,
        status: None,
    };

    // -- Exec
    let lines = layout(&block, &frame_ctx(80));

    // -- Check
    let text = lines_text(&lines);
    assert!(text.contains("compacted 5 blocks"), "got {text:?}");
}

#[test]
fn layout_spacer_renders_single_blank_line() {
    // -- Setup & Fixtures
    let block = Block {
        source: BlockSource::Spacer,
        lane: Lane::Blank,
        fill: Fill::None,
        status: None,
    };

    // -- Exec
    let lines = layout(&block, &frame_ctx(40));

    // -- Check
    assert_eq!(
        lines.len(),
        1,
        "spacer must be one row; got {}",
        lines.len()
    );
    let text = lines_text(&lines);
    assert!(text.trim().is_empty(), "spacer must be blank; got {text:?}");
}

#[test]
fn layout_banner_renders_every_line() {
    // -- Setup & Fixtures
    let banner = Banner {
        text: "line1\nline2".to_string(),
    };
    let block = Block {
        source: banner.source(),
        lane: banner.lane(),
        fill: banner.fill(),
        status: None,
    };

    // -- Exec
    let lines = layout(&block, &frame_ctx(80));

    // -- Check
    let text = lines_text(&lines);
    assert!(
        text.contains("line1") && text.contains("line2"),
        "got {text:?}"
    );
}

#[test]
fn layout_tool_in_progress_renders_content() {
    // -- Setup & Fixtures
    let block = Block {
        source: BlockSource::Tool {
            name: ToolName("tool".to_string()),
            call: CallLine {
                summary: "→ x".to_string(),
            },
            preview: None,
        },
        lane: Lane::Marker("⚙"),
        fill: Fill::None,
        status: Some(ItemStatus::InProgress),
    };

    // -- Exec
    let lines = layout(&block, &frame_ctx(80));

    // -- Check
    let text = lines_text(&lines);
    assert!(
        !text.trim().is_empty(),
        "in-progress tool must render; got {text:?}"
    );
}

#[test]
fn layout_notice_with_status_shows_indicator() {
    // -- Setup & Fixtures
    let block = Block {
        source: BlockSource::Notice {
            kind: NoticeKind::System,
            text: "note".to_string(),
        },
        lane: Lane::Marker("·"),
        fill: Fill::None,
        status: Some(ItemStatus::Done),
    };

    // -- Exec
    let lines = layout(&block, &frame_ctx(80));

    // -- Check
    let text = lines_text(&lines);
    assert!(
        text.contains("✓"),
        "done status must show checkmark; got {text:?}"
    );
    assert!(text.contains("note"), "got {text:?}");
}

fn lines_text(lines: &[ratatui::text::Line<'static>]) -> String {
    lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.content.as_ref())
        .collect()
}

// ========== measure() tests (task 53279c82) ==========

fn tool_code_block(code: &str) -> Block {
    let preview = Display {
        title: "nu".to_string(),
        sections: vec![DisplaySection {
            label: "nu".to_string(),
            kind: ContentKind::Code {
                language: "nu".to_string(),
            },
            content: code.to_string(),
            stats: None,
        }],
    };
    Block {
        source: BlockSource::Tool {
            name: ToolName("tool".to_string()),
            call: CallLine {
                summary: code.to_string(),
            },
            preview: Some(preview),
        },
        lane: Lane::Marker("⚙"),
        fill: Fill::Code,
        status: None,
    }
}

fn tool_diff_block(diff: &str) -> Block {
    let preview = Display {
        title: "edit a.rs".to_string(),
        sections: vec![DisplaySection {
            label: "a.rs".to_string(),
            kind: ContentKind::Diff {
                language: "diff".to_string(),
            },
            content: diff.to_string(),
            stats: None,
        }],
    };
    Block {
        source: BlockSource::Tool {
            name: ToolName("tool".to_string()),
            call: CallLine {
                summary: "→ a.rs (diff)".to_string(),
            },
            preview: Some(preview),
        },
        lane: Lane::Marker("⚙"),
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

#[test]
fn measure_markdown_matches_layout_row_count() {
    // -- Setup & Fixtures
    let block = markdown_block(MessageRole::User, "short line", Fill::None);
    let ctx = frame_ctx(80);

    // -- Exec
    let measured = measure(&block, 80);
    let laid_out = layout(&block, &ctx).len();

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

    // -- Exec
    let measured = measure(&block, 24);
    let laid_out = layout(&block, &ctx).len();

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

    // -- Exec
    let measured = measure(&block, 80);
    let laid_out = layout(&block, &ctx).len();

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

    // -- Exec
    let measured = measure(&block, 80);
    let laid_out = layout(&block, &ctx).len();

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

    // -- Exec
    let measured = measure(&block, 40);
    let laid_out = layout(&block, &ctx).len();

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

    // -- Exec
    let measured = measure(&block, 80);
    let laid_out = layout(&block, &ctx).len();

    // -- Check
    assert_eq!(
        measured, laid_out,
        "measure() must match layout() when a status indicator shrinks the row-0 budget"
    );
}

// ========== lane/row theme mapping tests (task 46ca79fe) ==========

/// Find the first rendered span whose text contains `needle`.
fn span_containing<'a>(
    lines: &'a [ratatui::text::Line<'static>],
    needle: &str,
) -> Option<&'a ratatui::text::Span<'static>> {
    lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .find(|span| span.content.contains(needle))
}

#[test]
fn layout_banner_text_uses_role_system_not_assistant() -> Result<()> {
    // -- Setup & Fixtures
    let theme = TuiTheme::default();
    let banner = Banner {
        text: "NUAGENT".to_string(),
    };
    let block = Block {
        source: banner.source(),
        lane: banner.lane(),
        fill: banner.fill(),
        status: None,
    };

    // -- Exec
    let lines = layout(&block, &frame_ctx(80));

    // -- Check
    let span = span_containing(&lines, "NUAGENT").ok_or("banner content span must exist")?;
    assert_eq!(
        span.style.fg, theme.role_system.fg,
        "banner text must use role_system fg, got {:?}",
        span.style.fg
    );
    assert_ne!(
        span.style.fg, theme.row_assistant.fg,
        "banner text must not use the assistant body fg"
    );
    assert_eq!(
        span.style.bg, theme.row_system.bg,
        "banner row background must match row_system"
    );
    Ok(())
}

#[test]
fn layout_assistant_markdown_content_uses_role_assistant() -> Result<()> {
    // -- Setup & Fixtures
    let theme = TuiTheme::default();
    let block = markdown_block(MessageRole::Assistant, "hello world", Fill::None);

    // -- Exec
    let lines = layout(&block, &frame_ctx(80));

    // -- Check
    let span = span_containing(&lines, "hello world").ok_or("assistant content span must exist")?;
    assert_eq!(
        span.style.fg, theme.role_assistant.fg,
        "assistant text must use role_assistant fg, got {:?}",
        span.style.fg
    );
    assert_ne!(
        span.style.fg, theme.row_assistant.fg,
        "assistant text must not use the row_assistant body fg"
    );
    Ok(())
}

#[test]
fn layout_tool_without_preview_row_bg_matches_row_tool() -> Result<()> {
    // -- Setup & Fixtures
    let theme = TuiTheme::default();
    let block = Block {
        source: BlockSource::Tool {
            name: ToolName("tool".to_string()),
            call: CallLine {
                summary: "\u{2192} a".to_string(),
            },
            preview: None,
        },
        lane: Lane::Marker("\u{2699}"),
        fill: Fill::None,
        status: None,
    };

    // -- Exec
    let lines = layout(&block, &frame_ctx(80));

    // -- Check
    let span = span_containing(&lines, "\u{2192} a").ok_or("tool call span must exist")?;
    assert_eq!(
        span.style.bg, theme.row_tool.bg,
        "tool row background must match row_tool"
    );
    Ok(())
}

#[test]
fn layout_compaction_notice_row_bg_matches_row_compaction() -> Result<()> {
    // -- Setup & Fixtures
    let theme = TuiTheme::default();
    let block = Block {
        source: BlockSource::Notice {
            kind: NoticeKind::Compaction,
            text: "compacted 5 blocks".to_string(),
        },
        lane: Lane::Marker("~"),
        fill: Fill::None,
        status: None,
    };

    // -- Exec
    let lines = layout(&block, &frame_ctx(80));

    // -- Check
    let span = span_containing(&lines, "compacted 5 blocks").ok_or("notice span must exist")?;
    assert_eq!(
        span.style.bg, theme.row_compaction.bg,
        "compaction row background must match row_compaction"
    );
    Ok(())
}

#[test]
fn layout_system_notice_row_bg_matches_row_system() -> Result<()> {
    // -- Setup & Fixtures
    let theme = TuiTheme::default();
    let block = Block {
        source: BlockSource::Notice {
            kind: NoticeKind::System,
            text: "system note".to_string(),
        },
        lane: Lane::Marker("\u{00b7}"),
        fill: Fill::None,
        status: None,
    };

    // -- Exec
    let lines = layout(&block, &frame_ctx(80));

    // -- Check
    let span = span_containing(&lines, "system note").ok_or("notice span must exist")?;
    assert_eq!(
        span.style.bg, theme.row_system.bg,
        "system notice row background must match row_system"
    );
    Ok(())
}

#[test]
fn layout_user_markdown_row_bg_matches_row_user_bg() -> Result<()> {
    // -- Setup & Fixtures
    let theme = TuiTheme::default();
    let block = markdown_block(MessageRole::User, "hi", Fill::Full);

    // -- Exec
    let lines = layout(&block, &frame_ctx(80));

    // -- Check
    let span = span_containing(&lines, "hi").ok_or("user content span must exist")?;
    assert_eq!(
        span.style.bg,
        Some(theme.row_user_bg),
        "user row background must match row_user_bg"
    );
    assert_eq!(
        span.style.bg,
        theme.row_user.bg(theme.row_user_bg).bg,
        "user row background must match row_user.bg(row_user_bg)"
    );
    Ok(())
}

// ========== Lane ownership of styling (task 46ca79fe) ==========

/// The lane decides styling, not the source: a Banner source on the assistant
/// `Lane::Blank` gets assistant styling. This pins the SRP fix —
/// `LaneContext::from` no longer inspects `block.source` to special-case
/// banners.
#[test]
fn layout_blank_lane_banner_uses_assistant_style_proving_lane_decides() -> Result<()> {
    // -- Setup & Fixtures
    let theme = TuiTheme::default();
    let block = Block {
        source: BlockSource::Banner {
            text: "NOTALOGO".to_string(),
        },
        lane: Lane::Blank,
        fill: Fill::None,
        status: None,
    };

    // -- Exec
    let lines = layout(&block, &frame_ctx(80));

    // -- Check
    let span = span_containing(&lines, "NOTALOGO").ok_or("banner content span must exist")?;
    assert_eq!(
        span.style.fg, theme.role_assistant.fg,
        "Lane::Blank must apply assistant styling regardless of the source; got {:?}",
        span.style.fg
    );
    Ok(())
}

/// `Lane::SystemBlank` applies system styling to its content: the banner
/// lane. `Banner::lane()` returns this variant, so the renderer needs no
/// source inspection to style a banner.
#[test]
fn layout_system_blank_lane_uses_role_system() -> Result<()> {
    // -- Setup & Fixtures
    let theme = TuiTheme::default();
    let block = Block {
        source: BlockSource::Banner {
            text: "LOGOART".to_string(),
        },
        lane: Lane::SystemBlank,
        fill: Fill::None,
        status: None,
    };

    // -- Exec
    let lines = layout(&block, &frame_ctx(80));

    // -- Check
    let span = span_containing(&lines, "LOGOART").ok_or("banner content span must exist")?;
    assert_eq!(
        span.style.fg, theme.role_system.fg,
        "Lane::SystemBlank must use role_system fg, got {:?}",
        span.style.fg
    );
    assert_eq!(
        span.style.bg, theme.row_system.bg,
        "Lane::SystemBlank row background must match row_system"
    );
    Ok(())
}

/// Every system-blank row carries the system row background, including all
/// banner lines.
#[test]
fn layout_system_blank_multi_line_banner_rows_use_row_system() -> Result<()> {
    // -- Setup & Fixtures
    let theme = TuiTheme::default();
    let banner = Banner {
        text: "line1\nline2".to_string(),
    };
    let block = Block {
        source: banner.source(),
        lane: banner.lane(),
        fill: banner.fill(),
        status: None,
    };
    assert_eq!(
        banner.lane(),
        Lane::SystemBlank,
        "banner lane is SystemBlank"
    );

    // -- Exec
    let lines = layout(&block, &frame_ctx(80));

    // -- Check
    for (idx, span) in lines.iter().flat_map(|l| &l.spans).enumerate() {
        assert_eq!(
            span.style.bg, theme.row_system.bg,
            "banner row {idx} span '{}' must use row_system bg, got {:?}",
            span.content, span.style.bg
        );
    }
    Ok(())
}

/// Centering follows the lane, not the source: a non-banner source on
/// `Lane::SystemBlank` is centered exactly like a banner. Proves the centering
/// decision reads `block.lane` and never inspects `block.source`.
#[test]
fn layout_non_banner_source_on_system_blank_lane_is_centered() -> Result<()> {
    // -- Setup & Fixtures: a notice (never centered by source) on the banner
    // lane.
    let block = Block {
        source: BlockSource::Notice {
            kind: NoticeKind::System,
            text: "CENTERME".to_string(),
        },
        lane: Lane::SystemBlank,
        fill: Fill::None,
        status: None,
    };

    // -- Exec
    let lines = layout(&block, &frame_ctx(80));

    // -- Check: centering prepends left padding, so row 0 carries more leading
    // whitespace than the bare lane prefix.
    let leading = leading_spaces(lines.first().ok_or("row 0 must exist")?);
    assert!(
        leading > lane_prefix_width(),
        "Lane::SystemBlank must center regardless of source; leading={leading}, prefix={}",
        lane_prefix_width()
    );
    Ok(())
}

/// Centering follows the lane, not the source: a banner source on the
/// assistant `Lane::Blank` is NOT centered. The lane already encodes "this is
/// a banner block" via `Lane::SystemBlank`.
#[test]
fn layout_banner_source_on_blank_lane_is_not_centered() -> Result<()> {
    // -- Setup & Fixtures
    let block = Block {
        source: BlockSource::Banner {
            text: "NOTCENTERED".to_string(),
        },
        lane: Lane::Blank,
        fill: Fill::None,
        status: None,
    };

    // -- Exec
    let lines = layout(&block, &frame_ctx(80));

    // -- Check: no centering padding is inserted, so the only leading
    // whitespace is the lane prefix itself.
    let leading = leading_spaces(lines.first().ok_or("row 0 must exist")?);
    assert_eq!(
        leading,
        lane_prefix_width(),
        "a Banner source on Lane::Blank must not be centered; leading={leading}"
    );
    Ok(())
}

/// A real banner block (source and lane from `Banner`) keeps its centering.
#[test]
fn layout_banner_on_system_blank_lane_is_centered() -> Result<()> {
    // -- Setup & Fixtures
    let banner = Banner {
        text: "LOGOLINE".to_string(),
    };
    let block = Block {
        source: banner.source(),
        lane: banner.lane(),
        fill: banner.fill(),
        status: None,
    };

    // -- Exec
    let lines = layout(&block, &frame_ctx(80));

    // -- Check
    let leading = leading_spaces(lines.first().ok_or("row 0 must exist")?);
    assert!(
        leading > lane_prefix_width(),
        "a banner block must stay centered; leading={leading}, prefix={}",
        lane_prefix_width()
    );
    Ok(())
}

/// Number of leading space characters on a rendered row.
fn leading_spaces(line: &ratatui::text::Line<'static>) -> usize {
    line.spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect::<String>()
        .chars()
        .take_while(|c| *c == ' ')
        .count()
}

/// Notice content is styled from the lane role style with `StyleHint::Normal`
/// (the pre-refactor `SystemMessage` styling), not `StyleHint::Meta`.
#[test]
fn layout_system_notice_content_uses_normal_hint_with_lane_role_style() -> Result<()> {
    // -- Setup & Fixtures
    let theme = TuiTheme::default();
    let block = Block {
        source: BlockSource::Notice {
            kind: NoticeKind::System,
            text: "system notice text".to_string(),
        },
        lane: Lane::Marker("\u{00b7}"),
        fill: Fill::None,
        status: None,
    };
    let expected = hint_to_style(&StyleHint::Normal, theme.role_system, &theme);

    // -- Exec
    let lines = layout(&block, &frame_ctx(80));

    // -- Check: the full style (fg + modifiers), not just the fg — `Meta`
    // shares a fg with some role styles but adds DIM.
    let span =
        span_containing(&lines, "system notice text").ok_or("notice content span must exist")?;
    assert_eq!(
        span.style, expected,
        "system notice content must use the Normal hint with role_system; got {:?}",
        span.style
    );
    assert_ne!(
        span.style,
        hint_to_style(&StyleHint::Meta, theme.role_system, &theme),
        "system notice content must not use the Meta hint"
    );
    Ok(())
}

/// Compaction notice content is styled from the compaction role style with
/// `StyleHint::Normal`.
#[test]
fn layout_compaction_notice_content_uses_normal_hint_with_lane_role_style() -> Result<()> {
    // -- Setup & Fixtures
    let theme = TuiTheme::default();
    let block = Block {
        source: BlockSource::Notice {
            kind: NoticeKind::Compaction,
            text: "compaction notice text".to_string(),
        },
        lane: Lane::Marker("~"),
        fill: Fill::None,
        status: None,
    };
    let expected = hint_to_style(&StyleHint::Normal, theme.role_compaction, &theme);

    // -- Exec
    let lines = layout(&block, &frame_ctx(80));

    // -- Check: full-style equality. `role_compaction` and `tool_meta` share
    // the same fg but differ by the DIM modifier, so only the full style
    // catches a Meta regression here.
    let span = span_containing(&lines, "compaction notice text")
        .ok_or("notice content span must exist")?;
    assert_eq!(
        span.style, expected,
        "compaction notice content must use the Normal hint with role_compaction; got {:?}",
        span.style
    );
    assert_ne!(
        span.style,
        hint_to_style(&StyleHint::Meta, theme.role_compaction, &theme),
        "compaction notice content must not use the Meta hint"
    );
    Ok(())
}

// ========== lane marker spacing tests (task 46502406) ==========

/// Build a Tool block with the given status; no preview.
fn tool_block_with_status(status: Option<ItemStatus>) -> Block {
    Block {
        source: BlockSource::Tool {
            name: ToolName("tool".to_string()),
            call: CallLine {
                summary: "\u{2192} a".to_string(),
            },
            preview: None,
        },
        lane: Lane::Marker("\u{2699}"),
        fill: Fill::None,
        status,
    }
}

/// The marker span is index 1 of the first rendered line (index 0 is the
/// 2-char cursor slot).
fn row_zero_marker<'a>(
    lines: &'a [ratatui::text::Line<'static>],
) -> Option<&'a ratatui::text::Span<'static>> {
    lines.first()?.spans.get(1)
}

#[test]
fn layout_tool_marker_and_done_indicator_have_space_between() -> Result<()> {
    // -- Setup & Fixtures
    let block = tool_block_with_status(Some(ItemStatus::Done));

    // -- Exec
    let lines = layout(&block, &frame_ctx(80));

    // -- Check
    let row0 = lines.first().ok_or("row 0 must exist")?;
    let text: String = row0.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(
        text.contains("\u{2699} \u{2713}"),
        "marker and checkmark must be space-separated; got {text:?}"
    );
    assert!(
        !text.contains("\u{2699}\u{2713}"),
        "marker and checkmark must not be adjacent; got {text:?}"
    );
    Ok(())
}

#[test]
fn layout_tool_marker_and_in_progress_indicator_have_space_between() -> Result<()> {
    // -- Setup & Fixtures: now_millis 0 selects spinner frame 0 (\u{280b}).
    let block = tool_block_with_status(Some(ItemStatus::InProgress));

    // -- Exec
    let lines = layout(&block, &frame_ctx(80));

    // -- Check
    let row0 = lines.first().ok_or("row 0 must exist")?;
    let text: String = row0.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(
        text.contains("\u{2699} \u{280b}"),
        "marker and spinner must be space-separated; got {text:?}"
    );
    assert!(
        !text.contains("\u{2699}\u{280b}"),
        "marker and spinner must not be adjacent; got {text:?}"
    );
    Ok(())
}

#[test]
fn layout_tool_marker_without_status_is_two_chars() -> Result<()> {
    // -- Setup & Fixtures
    let block = tool_block_with_status(None);

    // -- Exec
    let lines = layout(&block, &frame_ctx(80));

    // -- Check
    let marker = row_zero_marker(&lines).ok_or("marker span must exist")?;
    assert_eq!(
        marker.content.as_ref(),
        "\u{2699} ",
        "tool marker must be icon + trailing space"
    );
    Ok(())
}

#[test]
fn layout_every_marker_lane_renders_two_char_marker() -> Result<()> {
    // -- Setup & Fixtures
    let user = markdown_block(MessageRole::User, "hi", Fill::Full);
    let compaction = Block {
        source: BlockSource::Notice {
            kind: NoticeKind::Compaction,
            text: "c".to_string(),
        },
        lane: Lane::Marker("~"),
        fill: Fill::None,
        status: None,
    };
    let system = Block {
        source: BlockSource::Notice {
            kind: NoticeKind::System,
            text: "s".to_string(),
        },
        lane: Lane::Marker("\u{00b7}"),
        fill: Fill::None,
        status: None,
    };

    // -- Exec & Check
    for (block, expected) in [
        (&user, "\u{258f} "),
        (&compaction, "~ "),
        (&system, "\u{00b7} "),
    ] {
        let lines = layout(block, &frame_ctx(80));
        let marker = row_zero_marker(&lines).ok_or("marker span must exist")?;
        assert_eq!(
            marker.content.as_ref(),
            expected,
            "every marker lane must render icon + trailing space"
        );
    }
    Ok(())
}

#[test]
fn lane_prefix_width_matches_rendered_prefix_width() -> Result<()> {
    // -- Setup & Fixtures
    let block = tool_block_with_status(None);

    // -- Exec
    let lines = layout(&block, &frame_ctx(80));
    let row0 = lines.first().ok_or("row 0 must exist")?;
    let cursor = row0.spans.first().ok_or("cursor span must exist")?;
    let marker = row_zero_marker(&lines).ok_or("marker span must exist")?;
    let rendered_prefix = cursor.content.chars().count() + marker.content.chars().count();

    // -- Check
    assert_eq!(
        rendered_prefix,
        lane_prefix_width(),
        "lane_prefix_width() must equal the rendered cursor + marker width"
    );
    assert_eq!(lane_prefix_width(), 4, "cursor 2 + marker 2");
    Ok(())
}

// ========== lane prefix on continuation rows (task 667e4926) ==========

/// The 2-char lane marker span of any rendered row: index 0 is the
/// cursor/blank slot, index 1 is the marker slot (row 0 marker or the
/// continuation marker).
fn row_marker<'a>(
    line: &'a ratatui::text::Line<'static>,
) -> Option<&'a ratatui::text::Span<'static>> {
    line.spans.get(1)
}

/// WHEN `layout()` renders a user `BlockSource::Markdown` block with multiple
/// ContentLines, THE lane prefix span of every row SHALL contain `▏`.
#[test]
fn layout_user_multi_content_line_rows_all_carry_rail() -> Result<()> {
    // -- Setup & Fixtures: a soft break splits the paragraph into two
    // ContentLines, so the block renders as two rows at a wide width.
    let block = markdown_block(MessageRole::User, "first line\nsecond line", Fill::Full);

    // -- Exec
    let lines = layout(&block, &frame_ctx(80));

    // -- Check
    assert!(
        lines.len() >= 2,
        "fixture must render more than one row; got {}",
        lines.len()
    );
    for (idx, line) in lines.iter().enumerate() {
        let marker = row_marker(line).ok_or("marker span must exist")?;
        assert!(
            marker.content.contains('▏'),
            "row {idx} must carry the user rail; got {:?}",
            marker.content
        );
    }
    Ok(())
}

/// WHEN `layout()` renders a user `BlockSource::Markdown` block where the
/// first ContentLine wraps to multiple rows, THE lane prefix span of every
/// wrapped continuation row SHALL contain `▏`.
#[test]
fn layout_user_wrapped_continuation_rows_carry_rail() -> Result<()> {
    // -- Setup & Fixtures: wraps at width 24 (20-col budget after the prefix).
    let long_text = "aaaaaaaaaa bbbbbbbbbb cccccccccc dddddddddd";
    let block = markdown_block(MessageRole::User, long_text, Fill::Full);

    // -- Exec
    let lines = layout(&block, &frame_ctx(24));

    // -- Check
    assert!(
        lines.len() >= 2,
        "prose must wrap at width 24; got {}",
        lines.len()
    );
    for (idx, line) in lines.iter().enumerate() {
        let marker = row_marker(line).ok_or("marker span must exist")?;
        assert!(
            marker.content.contains('▏'),
            "wrapped row {idx} must carry the user rail; got {:?}",
            marker.content
        );
    }
    Ok(())
}

/// WHEN `layout()` renders a tool `BlockSource::Tool` block where content wraps
/// to multiple rows, THE lane prefix span of continuation rows SHALL NOT
/// contain `⚙` (blank continuation marker).
#[test]
fn layout_tool_wrapped_continuation_rows_have_blank_marker() -> Result<()> {
    // -- Setup & Fixtures: a long summary wraps at width 80.
    let block = Block {
        source: BlockSource::Tool {
            name: ToolName("tool".to_string()),
            call: CallLine {
                summary: format!("\u{2192} {}", "x".repeat(200)),
            },
            preview: None,
        },
        lane: Lane::Marker("⚙"),
        fill: Fill::None,
        status: None,
    };

    // -- Exec
    let lines = layout(&block, &frame_ctx(80));

    // -- Check: row 0 carries ⚙; every continuation row's marker is blank.
    assert!(lines.len() >= 2, "summary must wrap; got {}", lines.len());
    let row0 =
        row_marker(lines.first().ok_or("row 0 must exist")?).ok_or("marker span must exist")?;
    assert!(row0.content.contains('⚙'), "row 0 must carry the tool icon");
    for (idx, line) in lines.iter().enumerate().skip(1) {
        let marker = row_marker(line).ok_or("marker span must exist")?;
        assert!(
            !marker.content.contains('⚙'),
            "tool continuation row {idx} must have a blank marker; got {:?}",
            marker.content
        );
    }
    Ok(())
}

/// WHEN `layout()` renders a compaction `BlockSource::Notice` block where
/// content wraps to multiple rows, THE lane prefix span of continuation rows
/// SHALL NOT contain `~` (blank continuation marker).
#[test]
fn layout_compaction_wrapped_continuation_rows_have_blank_marker() -> Result<()> {
    // -- Setup & Fixtures: a long notice wraps at width 80.
    let block = Block {
        source: BlockSource::Notice {
            kind: NoticeKind::Compaction,
            text: "compact ".repeat(30),
        },
        lane: Lane::Marker("~"),
        fill: Fill::None,
        status: None,
    };

    // -- Exec
    let lines = layout(&block, &frame_ctx(80));

    // -- Check: row 0 carries ~; every continuation row's marker is blank.
    assert!(lines.len() >= 2, "notice must wrap; got {}", lines.len());
    let row0 =
        row_marker(lines.first().ok_or("row 0 must exist")?).ok_or("marker span must exist")?;
    assert!(
        row0.content.contains('~'),
        "row 0 must carry the compaction icon"
    );
    for (idx, line) in lines.iter().enumerate().skip(1) {
        let marker = row_marker(line).ok_or("marker span must exist")?;
        assert!(
            !marker.content.contains('~'),
            "compaction continuation row {idx} must have a blank marker; got {:?}",
            marker.content
        );
    }
    Ok(())
}

// ========== BlockSource::project integration (task 62e03e23) ==========

/// The renderer consumes `BlockSource::project()`: the content rendered after
/// the 2-span lane prefix is exactly the projected source's spans. Pins the
/// refactor (free function `project_source` → inherent `project`).
#[test]
fn layout_consumes_block_source_project_output() -> Result<()> {
    // -- Setup & Fixtures
    let block = Block {
        source: BlockSource::Tool {
            name: ToolName("edit".to_string()),
            call: CallLine {
                summary: "→ foo.rs".to_string(),
            },
            preview: None,
        },
        lane: Lane::Marker("⚙"),
        fill: Fill::None,
        status: None,
    };

    // -- Exec
    let projected = block.source.project(80);
    let lines = layout(&block, &frame_ctx(80));

    // -- Check: the projection is the renderer's content source.
    let proj_row0 = projected.first().ok_or("projection row 0 must exist")?;
    let proj_text: String = proj_row0.spans.iter().map(|s| s.text.as_str()).collect();
    let row0 = lines.first().ok_or("row 0 must exist")?;
    // Spans 0 and 1 are the lane prefix; the content spans follow.
    let rendered_text: String = row0
        .spans
        .iter()
        .skip(2)
        .map(|s| s.content.as_ref())
        .collect();
    assert_eq!(
        rendered_text, proj_text,
        "renderer content must be the projected source's spans"
    );
    Ok(())
}

/// The projection is reachable through the public method on a Markdown block
/// and produces the shared pipeline's text, which the renderer then displays.
#[test]
fn layout_markdown_content_is_project_output() -> Result<()> {
    // -- Setup & Fixtures
    let block = markdown_block(MessageRole::Assistant, "hello project", Fill::None);

    // -- Exec
    let projected = block.source.project(80);
    let lines = layout(&block, &frame_ctx(80));

    // -- Check
    let proj_text: String = projected
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.text.as_str())
        .collect();
    assert!(
        proj_text.contains("hello project"),
        "projection must contain the markdown text; got {proj_text:?}"
    );
    let rendered = lines_text(&lines);
    assert!(
        rendered.contains("hello project"),
        "rendered content must include the projected markdown; got {rendered:?}"
    );
    Ok(())
}

// ========== tool reducer effect tests (migrated) ==========
