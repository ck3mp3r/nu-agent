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
fn word_wrap_breaks_at_space() -> Result<()> {
    let r = make_renderer();
    // "hello world foobar" with width forcing a break between words.
    // Width = 20, prefix = 4, available = 16:
    // "hello world " = 12 chars, "foobar" = 6 chars -> wraps
    let text = "hello world foobar";
    let block = TranscriptEntry {
        id: 0,
        kind: TranscriptEntryKind::User(ProseMessage {
            markdown: text.to_string(),
        }),
        status: None,
    }
    .to_render_block();

    let lines = r.render(&block, &default_ctx(20));

    assert!(
        lines.len() > 1,
        "18-char prose at width 20 (16 after prefix) must wrap; got {} lines",
        lines.len()
    );

    let first_line = concat_spans(&[lines[0].clone()]);
    let second_line = concat_spans(&[lines[1].clone()]);

    // First line should contain "hello world" and NOT split mid-word
    assert!(
        first_line.contains("hello world"),
        "first line should contain complete words; got {first_line:?}"
    );

    // Second line should start with "foobar" (complete word, not a split)
    assert!(
        second_line.contains("foobar"),
        "continuation should contain complete word; got {second_line:?}"
    );

    // Verify no word is split with hyphen or mid-character
    assert!(
        !first_line.ends_with("foo"),
        "should not split 'foobar' into 'foo' and 'bar'"
    );

    // Continuation rows must carry the lane prefix: rows 1+ get the plain
    // lane prefix (no cursor, no status indicator), so the wrapped row
    // starts with the 4-col prefix before its content word.
    let second_prefix: String = lines[1]
        .spans
        .iter()
        .take(2)
        .map(|s| s.content.as_ref())
        .collect();
    assert_eq!(
        second_prefix, "  ▏ ",
        "wrapped continuation row must start with the user lane prefix; got {second_prefix:?}"
    );
    assert!(
        second_line.starts_with("  ▏ foobar"),
        "continuation row must be lane prefix followed by the content word; got {second_line:?}"
    );

    Ok(())
}

#[test]
fn wrapped_continuation_row_aligns_at_lane_prefix_column() -> Result<()> {
    // -- Setup & Fixtures
    let r = make_renderer();
    // 30-char single-line prose; at width 24 the available width after the
    // 4-col prefix is 20, so the text must wrap.
    let markdown = "aaaaaaaaaa bbbbbbbbbb cccccccccc";
    let block = TranscriptEntry {
        id: 0,
        kind: TranscriptEntryKind::User(ProseMessage {
            markdown: markdown.to_string(),
        }),
        status: None,
    }
    .to_render_block();

    // -- Exec
    let lines = r.render(&block, &default_ctx(24));

    // -- Check
    assert!(
        lines.len() >= 2,
        "30-char prose at width 24 (20 after prefix) must wrap; got {} lines",
        lines.len()
    );

    // Row 0: cursor/indicator lane prefix ("  " + "▏ ") then content.
    let first_prefix: String = lines[0]
        .spans
        .iter()
        .take(2)
        .map(|s| s.content.as_ref())
        .collect();
    assert_eq!(first_prefix, "  ▏ ", "first row must keep the lane prefix");

    // Every continuation row must start at column 4 — the same column as the
    // first content character of row 0 — because the lane prefix spans are
    // emitted on EVERY row, not just the first.
    for (row_idx, line) in lines.iter().enumerate().skip(1) {
        let prefix: String = line
            .spans
            .iter()
            .take(2)
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(
            prefix, "  ▏ ",
            "continuation row {row_idx} must start with the 4-col lane prefix, not wrap to column 0"
        );
        assert!(
            !line.spans.iter().any(|s| s.content.starts_with('a')),
            "continuation row {row_idx} must not begin with raw content before the prefix"
        );
    }

    // Content integrity: all three words survive the wrap, in order.
    let joined = concat_spans(&lines);
    assert!(
        joined.contains("aaaaaaaaaa")
            && joined.contains("bbbbbbbbbb")
            && joined.contains("cccccccccc"),
        "all words must survive wrapping; got {joined:?}"
    );

    Ok(())
}

// === Task 7bd175d2: continuation rows must not repeat the role icon ===

fn tool_wrapped_block(args_text: String) -> RenderBlock {
    TranscriptEntry {
        id: 0,
        kind: TranscriptEntryKind::Tool(ToolInvocation {
            name: "edit".to_string(),
            source: String::new(),
            args: args_text,
        }),
        status: None,
    }
    .to_render_block()
}

#[test]
fn wrapped_tool_row_continuation_uses_blank_label_with_lane_style() -> Result<()> {
    // -- Setup & Fixtures
    let r = make_renderer();
    // args line long enough to wrap at width 24 (20 available after prefix).
    let args = format!("→ {}", "a".repeat(60));
    let block = tool_wrapped_block(args);

    // -- Exec
    let lines = r.render(&block, &default_ctx(24));

    // -- Check
    assert!(
        lines.len() >= 2,
        "long tool args at width 24 must wrap; got {} lines",
        lines.len()
    );

    // Row 0 keeps the cog label.
    let first_prefix: String = lines[0]
        .spans
        .iter()
        .take(2)
        .map(|s| s.content.as_ref())
        .collect();
    assert_eq!(first_prefix, "  ⚙ ", "first row must keep the tool icon");

    // Continuation rows must NOT repeat the cog: their 2-char label slot is
    // blank, but the lane style must match the tool lane style.
    let theme = TuiTheme::default();
    for (row_idx, line) in lines.iter().enumerate().skip(1) {
        let prefix_spans: Vec<_> = line.spans.iter().take(2).collect();
        assert_eq!(
            prefix_spans.len(),
            2,
            "continuation row {row_idx} must still carry the 2-span prefix"
        );
        let label = prefix_spans[1].content.as_ref();
        assert_eq!(
            label, "  ",
            "continuation row {row_idx} label must be blank, got {label:?}"
        );
        assert_eq!(
            prefix_spans[1].style.fg, theme.lane_prefix_tool.fg,
            "continuation row {row_idx} label must keep the tool lane style"
        );
    }

    Ok(())
}

#[test]
fn wrapped_compaction_row_continuation_uses_blank_label() -> Result<()> {
    // -- Setup & Fixtures
    let r = make_renderer();
    let block = RenderBlock {
        role: Role::Compaction,
        lines: vec![ContentLine::single(
            format!("{} {}", "compact".repeat(6), "x".repeat(30)),
            StyleHint::Normal,
        )],
        markdown: None,
        center: false,
        suppress_prefix: false,
    };

    // -- Exec
    let lines = r.render(&block, &default_ctx(24));

    // -- Check
    assert!(lines.len() >= 2, "long compaction line must wrap");
    let second_prefix: String = lines[1]
        .spans
        .iter()
        .take(2)
        .map(|s| s.content.as_ref())
        .collect();
    assert_eq!(
        second_prefix, "    ",
        "compaction continuation row must have a blank label (cursor col + label col), got {second_prefix:?}"
    );
    Ok(())
}

#[test]
fn wrapped_system_row_continuation_uses_blank_label() -> Result<()> {
    // -- Setup & Fixtures
    let r = make_renderer();
    let block = RenderBlock {
        role: Role::System,
        lines: vec![ContentLine::single(
            format!("{} {}", "system".repeat(6), "y".repeat(30)),
            StyleHint::Normal,
        )],
        markdown: None,
        center: false,
        suppress_prefix: false,
    };

    // -- Exec
    let lines = r.render(&block, &default_ctx(24));

    // -- Check
    assert!(lines.len() >= 2, "long system line must wrap");
    let second_prefix: String = lines[1]
        .spans
        .iter()
        .take(2)
        .map(|s| s.content.as_ref())
        .collect();
    assert_eq!(
        second_prefix, "    ",
        "system continuation row must have a blank label, got {second_prefix:?}"
    );
    Ok(())
}

#[test]
fn wrapped_user_prose_keeps_rail_on_every_row() -> Result<()> {
    // Regression guard: user prose continuation rows must keep the ▏ rail.
    // -- Setup & Fixtures
    let r = make_renderer();
    let block = TranscriptEntry {
        id: 0,
        kind: TranscriptEntryKind::User(ProseMessage {
            markdown: format!("{} {}", "word ".repeat(10), "tail"),
        }),
        status: None,
    }
    .to_render_block();

    // -- Exec
    let lines = r.render(&block, &default_ctx(24));

    // -- Check
    assert!(lines.len() >= 2, "long user prose must wrap");
    for (row_idx, line) in lines.iter().enumerate() {
        let prefix: String = line
            .spans
            .iter()
            .take(2)
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(
            prefix, "  ▏ ",
            "user row {row_idx} must keep the ▏ rail on every wrapped row"
        );
    }
    Ok(())
}

// === Task 7bd175d2: list item hanging indent ===

#[test]
fn wrapped_list_item_continuation_indents_under_marker_text() -> Result<()> {
    // -- Setup & Fixtures
    let r = make_renderer();
    // A bullet item whose text wraps: the "- " marker projects as "• " (2 cols).
    let markdown = "- aaa bbb ccc ddd eee fff ggg hhh iii jjj kkk lll mmm nnn ooo ppp";
    let block = TranscriptEntry {
        id: 0,
        kind: TranscriptEntryKind::Assistant(ProseMessage {
            markdown: markdown.to_string(),
        }),
        status: None,
    }
    .to_render_block();

    // -- Exec
    let lines = r.render(&block, &default_ctx(24));

    // -- Check
    assert!(
        lines.len() >= 2,
        "long list item must wrap; got {} lines",
        lines.len()
    );

    // Row 0: lane prefix (assistant = 4 blank cols) + "• a..."
    let first_text = concat_spans(&[lines[0].clone()]);
    assert!(
        first_text.len() > 4 && first_text[4..].starts_with("• "),
        "first row must contain the bullet marker after the 4-col lane prefix; got {first_text:?}"
    );

    // Continuation rows: lane prefix + 2 spaces of hang indent + item text.
    for (row_idx, line) in lines.iter().enumerate().skip(1) {
        let text = concat_spans(std::slice::from_ref(line));
        assert!(
            text.starts_with("      "),
            "assistant list continuation row {row_idx} must indent to marker column + marker width (6 cols total); got {text:?}"
        );
    }
    Ok(())
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
