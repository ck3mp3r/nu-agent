use super::support::{frame_ctx, markdown_block, span_containing};
use crate::rendering::theme::TuiTheme;
use crate::tui_renderer::layout;
use nu_agent_core::protocol::tool_args::CallLine;
use nu_agent_core::transcript::ir::*;
use nu_agent_core::transcript::items::Banner;
use nu_agent_core::transcript::renderer::Renderable;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ========== lane/row theme mapping tests (task 46ca79fe) ==========

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
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

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
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

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
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

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
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

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
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

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
    let lines = layout(&block, &frame_ctx(80), &TuiTheme::default());

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
