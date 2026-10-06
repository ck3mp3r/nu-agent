use super::support::{frame_ctx, lines_text, markdown_block};
use crate::rendering::theme::{TuiTheme, hint_to_style};
use crate::tui_renderer::layout;
use nu_agent_core::protocol::tool_args::CallLine;
use nu_agent_core::transcript::ir::*;
use nu_agent_core::transcript::items::Banner;
use nu_agent_core::transcript::renderer::{ItemStatus, Renderable};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ========== layout() tests (task 6a7a3916) ==========

#[test]
fn layout_markdown_fill_none_has_no_background() {
    // -- Setup & Fixtures
    let block = markdown_block(MessageRole::Assistant, "hello", Fill::None);

    // -- Exec
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

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
    let lines = layout(&block, &frame_ctx(24), &TuiTheme::default());

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
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

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
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

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
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

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
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

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
    let lines = layout(&block, &frame_ctx(40), &TuiTheme::default());

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
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

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
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

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
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

    // -- Check
    let text = lines_text(&lines);
    assert!(
        text.contains("✓"),
        "done status must show checkmark; got {text:?}"
    );
    assert!(text.contains("note"), "got {text:?}");
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
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

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
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

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
