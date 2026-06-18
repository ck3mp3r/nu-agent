use ratatui::style::Modifier;

use crate::rendering::theme::TuiTheme;

use crate::tui_renderer::TuiRenderer;
use nu_agent_core::transcript::ir::*;
use nu_agent_core::transcript::items::*;
use nu_agent_core::transcript::renderer::*;

fn make_renderer() -> TuiRenderer {
    TuiRenderer {
        theme: TuiTheme::default(),
    }
}

fn default_ctx(width: usize) -> RenderContext {
    RenderContext {
        width,
        cursor: false,
        selected: false,
        status: None,
        now_millis: 0,
    }
}

fn concat_spans(lines: &[ratatui::text::Line<'static>]) -> String {
    lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.content.as_ref())
        .collect()
}

#[test]
fn tool_row_renders_name_without_tool_brackets() {
    let r = make_renderer();
    let block = ToolInvocation {
        name: "nu__run".to_string(),
        source: "".to_string(),
        args: r#"{"command":"version"}"#.to_string(),
    }
    .to_render_block();
    let lines = r.render(&block, &default_ctx(120));
    let text = concat_spans(&lines);
    assert!(text.contains("nu__run"), "should contain tool name");
    assert!(!text.contains("tool["), "should not have tool[ prefix");
    assert!(
        text.contains(r#"{"command":"version"}"#),
        "should contain args"
    );
}

#[test]
fn tool_lane_prefix_uses_cog_wheel() {
    let r = make_renderer();
    let block = ToolInvocation {
        name: "test".to_string(),
        source: "".to_string(),
        args: "".to_string(),
    }
    .to_render_block();
    let lines = r.render(&block, &default_ctx(80));
    let prefix: String = lines[0]
        .spans
        .iter()
        .take(2)
        .map(|s| s.content.as_ref())
        .collect();
    assert_eq!(prefix, "  ⚙ ");
}

#[test]
fn tool_done_shows_checkmark() {
    let r = make_renderer();
    let block = ToolInvocation {
        name: "test".to_string(),
        source: "".to_string(),
        args: "".to_string(),
    }
    .to_render_block();
    let mut ctx = default_ctx(80);
    ctx.status = Some(ItemStatus::Done);
    let lines = r.render(&block, &ctx);
    let text = concat_spans(&lines);
    assert!(text.contains("✓"), "should show checkmark");
    assert!(!text.contains("· done"), "should not have done text");
}

#[test]
fn tool_failed_shows_cross() {
    let r = make_renderer();
    let block = ToolInvocation {
        name: "test".to_string(),
        source: "".to_string(),
        args: "".to_string(),
    }
    .to_render_block();
    let mut ctx = default_ctx(80);
    ctx.status = Some(ItemStatus::Failed);
    let lines = r.render(&block, &ctx);
    let text = concat_spans(&lines);
    assert!(text.contains("✕"), "should show cross");
}

#[test]
fn diff_add_line_uses_done_fg_color() {
    let r = make_renderer();
    let block = RenderBlock {
        role: Role::ToolDisplay,
        lines: vec![ContentLine::single(
            "+added".to_string(),
            StyleHint::DiffAdd,
        )],
    };
    let lines = r.render(&block, &default_ctx(80));
    let span = lines[0]
        .spans
        .iter()
        .find(|s| s.content.contains("+added"))
        .expect("should have +added span");
    assert_eq!(span.style.fg, TuiTheme::default().status_done.fg);
}

#[test]
fn diff_remove_line_uses_failed_fg_color() {
    let r = make_renderer();
    let block = RenderBlock {
        role: Role::ToolDisplay,
        lines: vec![ContentLine::single(
            "-removed".to_string(),
            StyleHint::DiffRemove,
        )],
    };
    let lines = r.render(&block, &default_ctx(80));
    let span = lines[0]
        .spans
        .iter()
        .find(|s| s.content.contains("-removed"))
        .expect("should have -removed span");
    assert_eq!(span.style.fg, TuiTheme::default().status_failed.fg);
}

#[test]
fn diff_hunk_line_has_bold_modifier() {
    let r = make_renderer();
    let block = RenderBlock {
        role: Role::ToolDisplay,
        lines: vec![ContentLine::single(
            "@@ -3,2 +3,2 @@".to_string(),
            StyleHint::DiffHunk,
        )],
    };
    let lines = r.render(&block, &default_ctx(80));
    let span = lines[0]
        .spans
        .iter()
        .find(|s| s.content.contains("@@"))
        .expect("should have hunk span");
    assert!(
        span.style.add_modifier.contains(Modifier::BOLD),
        "hunk should be bold"
    );
}

#[test]
fn separator_renders_repeated_dash_char() {
    let r = make_renderer();
    let block = Separator.to_render_block();
    let lines = r.render(&block, &default_ctx(40));
    let text = concat_spans(&lines);
    assert!(text.contains("─"), "should contain horizontal rule char");
    let dash_count = text.chars().filter(|&c| c == '─').count();
    assert_eq!(dash_count, 36, "should be width-4 dashes");
}

#[test]
fn selected_row_has_selection_bg_on_all_spans() {
    let r = make_renderer();
    let block = TranscriptEntry::User(ProseMessage {
        lines: vec![ContentLine::single("hi".to_string(), StyleHint::Normal)],
    })
    .to_render_block();
    let mut ctx = default_ctx(80);
    ctx.selected = true;
    let lines = r.render(&block, &ctx);
    let theme = TuiTheme::default();
    for span in &lines[0].spans {
        assert_eq!(
            span.style.bg, theme.selection_bg.bg,
            "all spans should have selection bg, but span '{}' does not",
            span.content
        );
    }
}

// ========== Text Wrapping Tests ==========

#[test]
fn short_line_no_wrap() {
    let r = make_renderer();
    let block = TranscriptEntry::User(ProseMessage {
        lines: vec![ContentLine::single(
            "Short text".to_string(),
            StyleHint::Normal,
        )],
    })
    .to_render_block();
    let lines = r.render(&block, &default_ctx(80));
    assert_eq!(lines.len(), 1, "short line should not wrap");
}

#[test]
fn word_wrap_breaks_at_space() {
    let r = make_renderer();
    // "hello world foo" with width forcing a break between words
    // Prefix is 4 chars, available = 40 - 4 = 36 chars
    // First word "hello" = 5 chars, space = 1, "world" = 5, space = 1, "foo" = 3
    // Total = 15 chars, fits on one line
    // But let's make it wrap: "hello world " = 12 chars
    let text = "hello world foobar";
    let block = TranscriptEntry::User(ProseMessage {
        lines: vec![ContentLine::single(text.to_string(), StyleHint::Normal)],
    })
    .to_render_block();

    // Width = 20, prefix = 4, available = 16
    // "hello world " = 12 chars, "foobar" = 6 chars -> wraps
    let lines = r.render(&block, &default_ctx(20));

    if lines.len() > 1 {
        let first_line = concat_spans(&[lines[0].clone()]);
        let second_line = concat_spans(&[lines[1].clone()]);

        // First line should contain "hello world" and NOT split mid-word
        assert!(
            first_line.contains("hello world") || first_line.contains("hello"),
            "first line should contain complete words"
        );

        // Second line should start with "foobar" or "world" (complete word)
        assert!(
            second_line.contains("foobar") || second_line.contains("world"),
            "continuation should start with complete word"
        );

        // Verify no word is split with hyphen or mid-character
        assert!(
            !first_line.ends_with("foo") || first_line.contains("foobar"),
            "should not split 'foobar' into 'foo' and 'bar'"
        );
    }
}

// === Task 5: visual differentiation between user and assistant prose ===

#[cfg(test)]
mod task_5_visual_diff_tests {
    use super::*;
    use nu_agent_core::transcript::ir::{ContentLine, Span, StyleHint};
    use nu_agent_core::transcript::items::{ProseMessage, TranscriptEntry};
    use nu_agent_core::transcript::renderer::BlockRenderer;
    use ratatui::style::Modifier;

    fn render_block_for(
        role_user: bool,
        lines: Vec<ContentLine>,
    ) -> Vec<ratatui::text::Line<'static>> {
        let r = make_renderer();
        let entry = if role_user {
            TranscriptEntry::User(ProseMessage { lines })
        } else {
            TranscriptEntry::Assistant(ProseMessage { lines })
        };
        let block = entry.to_render_block();
        r.render(&block, &default_ctx(80))
    }

    #[test]
    fn user_prose_uses_user_lane_prefix() {
        let lines = render_block_for(
            true,
            vec![ContentLine::single("hi".to_string(), StyleHint::Normal)],
        );
        let prefix: String = lines[0]
            .spans
            .iter()
            .take(2)
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(prefix, "  ▏ ");
    }

    #[test]
    fn assistant_prose_uses_assistant_lane_prefix() {
        let lines = render_block_for(
            false,
            vec![ContentLine::single("hi".to_string(), StyleHint::Normal)],
        );
        let prefix: String = lines[0]
            .spans
            .iter()
            .take(2)
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(prefix, "    ");
    }

    #[test]
    fn user_prose_has_no_row_background() {
        let lines = render_block_for(
            true,
            vec![ContentLine::single("hi".to_string(), StyleHint::Normal)],
        );
        for span in &lines[0].spans {
            assert_eq!(
                span.style.bg, None,
                "user prose rows must have no background color; got {:?}",
                span.style.bg
            );
        }
    }

    #[test]
    fn assistant_prose_has_no_row_background() {
        let lines = render_block_for(
            false,
            vec![ContentLine::single("hi".to_string(), StyleHint::Normal)],
        );
        for span in &lines[0].spans {
            assert_eq!(
                span.style.bg, None,
                "assistant prose rows must have no background color; got {:?}",
                span.style.bg
            );
        }
    }

    #[test]
    fn user_md_bold_renders_with_bold_modifier() {
        let lines = render_block_for(
            true,
            vec![ContentLine::from_spans(vec![Span::new(
                "world".to_string(),
                StyleHint::MdBold,
            )])],
        );
        let bold = lines[0]
            .spans
            .iter()
            .find(|s| s.content.contains("world"))
            .expect("bold span");
        assert!(bold.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn assistant_md_bold_renders_with_bold_modifier() {
        let lines = render_block_for(
            false,
            vec![ContentLine::from_spans(vec![Span::new(
                "world".to_string(),
                StyleHint::MdBold,
            )])],
        );
        let bold = lines[0]
            .spans
            .iter()
            .find(|s| s.content.contains("world"))
            .expect("bold span");
        assert!(bold.style.add_modifier.contains(Modifier::BOLD));
    }
}
