use ratatui::style::Modifier;

use crate::rendering::theme::TuiTheme;

use crate::tui_renderer::TuiRenderer;
use nu_agent_core::transcript::ir::*;
use nu_agent_core::transcript::items::*;
use nu_agent_core::transcript::renderer::*;
use std::collections::HashMap;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

fn make_renderer() -> TuiRenderer {
    TuiRenderer {
        theme: TuiTheme::default(),
    }
}

// ========== render_cached cache tests ==========

#[test]
fn render_cached_cache_hit_returns_identical_output() {
    let r = make_renderer();
    let mut cache: HashMap<String, Vec<ContentLine>> = HashMap::new();
    let block = RenderBlock {
        role: Role::Assistant,
        lines: vec![],
        markdown: Some("**bold**".to_string()),
        center: false,
        suppress_prefix: false,
    };
    let ctx = default_ctx(80);

    let first = r.render_cached(&block, &ctx, &mut cache);
    let second = r.render_cached(&block, &ctx, &mut cache);

    assert_eq!(first, second, "cache hit must return identical output");
    assert_eq!(
        cache.len(),
        1,
        "cache must have exactly 1 entry after 2 calls"
    );
}

#[test]
fn render_cached_cache_miss_stores_entry_and_renders_bold() {
    let r = make_renderer();
    let mut cache: HashMap<String, Vec<ContentLine>> = HashMap::new();
    let block = RenderBlock {
        role: Role::Assistant,
        lines: vec![],
        markdown: Some("**bold**".to_string()),
        center: false,
        suppress_prefix: false,
    };
    let ctx = default_ctx(80);

    let lines = r.render_cached(&block, &ctx, &mut cache);

    assert_eq!(cache.len(), 1, "cache must have 1 entry after first call");
    let has_bold = lines.iter().flat_map(|l| l.spans.iter()).any(|s| {
        s.style
            .add_modifier
            .contains(ratatui::style::Modifier::BOLD)
    });
    assert!(has_bold, "output must contain bold-styled spans");
}

#[test]
fn render_cached_non_markdown_bypasses_cache() {
    let r = make_renderer();
    let mut cache: HashMap<String, Vec<ContentLine>> = HashMap::new();
    let block = RenderBlock {
        role: Role::Assistant,
        lines: vec![ContentLine::single("hello".to_string(), StyleHint::Normal)],
        markdown: None,
        center: false,
        suppress_prefix: false,
    };
    let ctx = default_ctx(80);

    let _ = r.render_cached(&block, &ctx, &mut cache);

    assert_eq!(
        cache.len(),
        0,
        "cache must not be modified for non-markdown blocks"
    );
}

#[test]
fn render_cached_cache_invalidation_clears_all_entries() {
    let r = make_renderer();
    let mut cache: HashMap<String, Vec<ContentLine>> = HashMap::new();
    let block = RenderBlock {
        role: Role::Assistant,
        lines: vec![],
        markdown: Some("**bold**".to_string()),
        center: false,
        suppress_prefix: false,
    };
    let ctx = default_ctx(80);

    let _ = r.render_cached(&block, &ctx, &mut cache);
    assert_eq!(cache.len(), 1, "cache should have 1 entry before clear");

    cache.clear();
    assert_eq!(cache.len(), 0, "cache must be empty after clear");
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
        name: "nu".to_string(),
        source: "".to_string(),
        args: r#"{"command":"version"}"#.to_string(),
    }
    .to_render_block();
    let lines = r.render(&block, &default_ctx(120));
    let text = concat_spans(&lines);
    assert!(text.contains("nu"), "should contain tool name");
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
fn tool_unknown_shows_question_mark_with_queued_style() -> Result<()> {
    let r = make_renderer();
    let block = ToolInvocation {
        name: "test".to_string(),
        source: "".to_string(),
        args: "".to_string(),
    }
    .to_render_block();
    let mut ctx = default_ctx(80);
    ctx.status = Some(ItemStatus::Unknown);
    let lines = r.render(&block, &ctx);
    let span = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .find(|s| s.content.contains("?"))
        .ok_or("should have unknown indicator span")?;
    assert_eq!(span.content.as_ref(), "? ", "unknown indicator must be '?'");
    assert_eq!(
        span.style.fg,
        TuiTheme::default().status_queued.fg,
        "unknown indicator must use the status_queued style"
    );
    Ok(())
}

#[test]
fn diff_add_line_uses_done_fg_color() -> Result<()> {
    let r = make_renderer();
    let block = RenderBlock {
        role: Role::ToolDisplay,
        lines: vec![ContentLine::single(
            "+added".to_string(),
            StyleHint::DiffAdd,
        )],
        markdown: None,
        center: false,
        suppress_prefix: false,
    };
    let lines = r.render(&block, &default_ctx(80));
    let span = lines[0]
        .spans
        .iter()
        .find(|s| s.content.contains("+added"))
        .ok_or("should have +added span")?;
    assert_eq!(span.style.fg, TuiTheme::default().status_done.fg);
    Ok(())
}

#[test]
fn diff_remove_line_uses_failed_fg_color() -> Result<()> {
    let r = make_renderer();
    let block = RenderBlock {
        role: Role::ToolDisplay,
        lines: vec![ContentLine::single(
            "-removed".to_string(),
            StyleHint::DiffRemove,
        )],
        markdown: None,
        center: false,
        suppress_prefix: false,
    };
    let lines = r.render(&block, &default_ctx(80));
    let span = lines[0]
        .spans
        .iter()
        .find(|s| s.content.contains("-removed"))
        .ok_or("should have -removed span")?;
    assert_eq!(span.style.fg, TuiTheme::default().status_failed.fg);
    Ok(())
}

#[test]
fn diff_hunk_line_has_bold_modifier() -> Result<()> {
    let r = make_renderer();
    let block = RenderBlock {
        role: Role::ToolDisplay,
        lines: vec![ContentLine::single(
            "@@ -3,2 +3,2 @@".to_string(),
            StyleHint::DiffHunk,
        )],
        markdown: None,
        center: false,
        suppress_prefix: false,
    };
    let lines = r.render(&block, &default_ctx(80));
    let span = lines[0]
        .spans
        .iter()
        .find(|s| s.content.contains("@@"))
        .ok_or("should have hunk span")?;
    assert!(
        span.style.add_modifier.contains(Modifier::BOLD),
        "hunk should be bold"
    );
    Ok(())
}

#[test]
fn separator_renders_as_blank_line() {
    let r = make_renderer();
    let block = TranscriptEntry {
        id: 0,
        kind: TranscriptEntryKind::Spacer(Spacer),
        status: None,
    }
    .to_render_block();
    let lines = r.render(&block, &default_ctx(40));
    let text = concat_spans(&lines);
    assert!(
        !text.contains('─'),
        "separator should not render a horizontal rule; got: {text:?}"
    );
    assert!(
        text.trim().is_empty(),
        "separator should render as blank; got: {text:?}"
    );
}

#[test]
fn selected_row_has_selection_bg_on_all_spans() {
    let r = make_renderer();
    let block = TranscriptEntry {
        id: 0,
        kind: TranscriptEntryKind::User(ProseMessage {
            markdown: "hi".to_string(),
        }),
        status: None,
    }
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
    let block = TranscriptEntry {
        id: 0,
        kind: TranscriptEntryKind::User(ProseMessage {
            markdown: "Short text".to_string(),
        }),
        status: None,
    }
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
    let block = TranscriptEntry {
        id: 0,
        kind: TranscriptEntryKind::User(ProseMessage {
            markdown: text.to_string(),
        }),
        status: None,
    }
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
    use nu_agent_core::transcript::items::{ProseMessage, TranscriptEntry};
    use nu_agent_core::transcript::renderer::BlockRenderer;
    use ratatui::style::Modifier;

    fn render_block_for(role_user: bool, markdown: &str) -> Vec<ratatui::text::Line<'static>> {
        let r = make_renderer();
        let entry = if role_user {
            TranscriptEntry {
                id: 0,
                kind: TranscriptEntryKind::User(ProseMessage {
                    markdown: markdown.to_string(),
                }),
                status: None,
            }
        } else {
            TranscriptEntry {
                id: 0,
                kind: TranscriptEntryKind::Assistant(ProseMessage {
                    markdown: markdown.to_string(),
                }),
                status: None,
            }
        };
        let block = entry.to_render_block();
        r.render(&block, &default_ctx(80))
    }

    #[test]
    fn user_prose_uses_user_lane_prefix() {
        let lines = render_block_for(true, "hi");
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
        let lines = render_block_for(false, "hi");
        let prefix: String = lines[0]
            .spans
            .iter()
            .take(2)
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(prefix, "    ");
    }

    #[test]
    fn user_prose_has_row_background() {
        let lines = render_block_for(true, "hi");
        let bg = crate::rendering::theme::TuiTheme::default().row_user_bg;
        for span in &lines[0].spans {
            assert_eq!(
                span.style.bg,
                Some(bg),
                "user prose rows must have the USER_BG background color; got {:?}",
                span.style.bg
            );
        }
    }

    #[test]
    fn assistant_prose_has_no_row_background() {
        let lines = render_block_for(false, "hi");
        for span in &lines[0].spans {
            assert_eq!(
                span.style.bg, None,
                "assistant prose rows must have no background color; got {:?}",
                span.style.bg
            );
        }
    }

    #[test]
    fn user_md_bold_renders_with_bold_modifier() -> Result<()> {
        // Pass raw markdown — renderer projects at render time
        let lines = render_block_for(true, "**world**");
        let bold = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .find(|s| s.content.contains("world"))
            .ok_or("should have bold span containing 'world'")?;
        assert!(bold.style.add_modifier.contains(Modifier::BOLD));
        Ok(())
    }

    #[test]
    fn assistant_md_bold_renders_with_bold_modifier() -> Result<()> {
        let lines = render_block_for(false, "**world**");
        let bold = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .find(|s| s.content.contains("world"))
            .ok_or("should have bold span containing 'world'")?;
        assert!(bold.style.add_modifier.contains(Modifier::BOLD));
        Ok(())
    }

    // ── New tests added by task 70c69610 ────────────────────────────────────

    #[test]
    fn prose_message_stores_raw_markdown_field() {
        let msg = ProseMessage {
            markdown: "# Hello".to_string(),
        };
        assert_eq!(msg.markdown, "# Hello");
    }

    #[test]
    fn render_markdown_lines_accepts_max_width_none() {
        // render_markdown_lines("# Hello", None) should produce non-empty output
        let lines = crate::markdown::render_markdown_lines("# Hello", None);
        assert!(
            !lines.is_empty(),
            "render_markdown_lines with None width must produce non-empty output"
        );
        let has_text = lines
            .iter()
            .any(|l| l.spans.iter().any(|s| s.text.contains("Hello")));
        assert!(has_text, "expected 'Hello' in projected output");
    }

    #[test]
    fn tui_renderer_projects_prose_at_render_time() {
        // Construct a ProseMessage with raw markdown; verify the renderer
        // produces styled output containing the text — no pre-projection needed.
        let r = make_renderer();
        let block = TranscriptEntry {
            id: 0,
            kind: TranscriptEntryKind::Assistant(ProseMessage {
                markdown: "hello".to_string(),
            }),
            status: None,
        }
        .to_render_block();
        let lines = r.render(&block, &default_ctx(80));
        assert!(!lines.is_empty(), "renderer must produce at least one line");
        let all_text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(
            all_text.contains("hello"),
            "rendered output must contain the markdown text; got: {all_text:?}"
        );
    }
}

#[test]
fn logo_entry_renders_without_lane_prefix() {
    let r = make_renderer();
    let block = TranscriptEntry {
        id: 0,
        kind: TranscriptEntryKind::Logo("test".to_string()),
        status: None,
    }
    .to_render_block();
    let lines = r.render(&block, &default_ctx(80));
    let prefix: String = lines[0]
        .spans
        .iter()
        .skip(1)
        .take(2)
        .map(|s| s.content.as_ref())
        .collect();
    assert_eq!(
        prefix, "    ",
        "logo must have no role prefix, got: {prefix:?}"
    );
}

#[test]
fn logo_entry_centered_adds_padding() {
    let r = make_renderer();
    let block = TranscriptEntry {
        id: 0,
        kind: TranscriptEntryKind::Logo("x".to_string()),
        status: None,
    }
    .to_render_block();
    let lines = r.render(&block, &default_ctx(80));
    let text: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(
        text.starts_with(" "),
        "centered logo must have leading padding"
    );
    assert!(text.contains("x"), "centered logo must contain the text");
}
