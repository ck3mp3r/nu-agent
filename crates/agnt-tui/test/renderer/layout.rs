//! Family-level rendering: what each entry family paints on its lane.

use super::support::*;
use crate::state::code_block::content_wrap_width;
use agnt_core::transcript::ir::MessageRole;
use agnt_core::transcript::renderer::ItemStatus;
use agnt_render::catalog::LaneKind;

// region:    --- Prose

/// WHEN the assistant prose renders, THE rows SHALL carry no background: prose
/// is untinted, and the pane supplies the surface underneath it.
#[test]
fn test_renderer_layout_assistant_prose_is_untinted() -> Result<()> {
    // -- Setup & Fixtures
    let entry = message_entry(MessageRole::Assistant, "hello there");

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check
    assert_eq!(rows, 1);
    for x in 0..WIDTH {
        assert!(
            is_untinted(cell_bg(&backend, x, 0)),
            "column {x} must carry no background"
        );
    }
    assert!(row_trimmed(&backend, 0).contains("hello there"));
    Ok(())
}

/// WHEN the user prose renders, EVERY row SHALL paint the user surface across
/// the pane's width, wrapped rows included.
#[test]
fn test_renderer_layout_user_prose_paints_the_user_surface() -> Result<()> {
    // -- Setup & Fixtures
    let theme = test_theme();
    let entry = message_entry(MessageRole::User, &long_prose());

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check
    assert!(rows >= 2, "the prose must wrap; got {rows}");
    for y in 0..u16::try_from(rows)? {
        for x in [0u16, 1, WIDTH / 2, WIDTH - 1] {
            assert_eq!(
                cell_bg(&backend, x, y),
                Some(terminal(theme.user_surface)),
                "row {y} column {x} must paint the user surface"
            );
        }
    }
    Ok(())
}

/// WHEN a coalesced user turn holds several prompts, THE widget SHALL paint them
/// as one turn: the soft-break join SHALL cost fewer rows than the paragraph form
/// — no separator row per prompt.
#[test]
fn test_renderer_layout_coalesced_user_turn_paints_one_row_per_prompt() -> Result<()> {
    // -- Setup & Fixtures: three single-line prompts, joined the way the drain
    // joins them.
    let messages = ["first message", "second message", "third message"];
    let coalesced = message_entry(MessageRole::User, &messages.join("\n"));
    let paragraph_separated = message_entry(MessageRole::User, &messages.join("\n\n"));

    // -- Exec
    let (joined, joined_rows) = render_at(&coalesced, 0)?;
    let (_, separated_rows) = render_at(&paragraph_separated, 0)?;

    // -- Check: every prompt renders, in order, and the join costs no separator
    // row per prompt.
    let text: String = (0..u16::try_from(joined_rows)?)
        .map(|y| row_text(&joined, y))
        .collect::<Vec<String>>()
        .join(" ");
    let mut cursor = 0usize;
    for message in messages {
        let found = text[cursor..]
            .find(message)
            .ok_or("every coalesced prompt must render, in order")?;
        cursor += found + message.len();
    }
    assert_eq!(
        joined_rows,
        messages.len(),
        "the soft-break join paints exactly one row per prompt"
    );
    assert!(
        joined_rows < separated_rows,
        "the soft-break join ({joined_rows} rows) must cost fewer rows than the paragraph form ({separated_rows})"
    );
    assert_eq!(
        separated_rows,
        messages.len() * 2 - 1,
        "the paragraph join inserts a separator row between prompts"
    );
    Ok(())
}

// endregion: --- Prose

// region:    --- Tool call line

/// WHEN a tool call has no display body, THE widget SHALL paint the call line
/// alone: the marker, the derived call line, and no body row.
#[test]
fn test_renderer_layout_tool_without_display_paints_call_line_only() -> Result<()> {
    // -- Setup & Fixtures
    let entry = tool_entry("read", r#"{"path":"a.rs"}"#, None, None, ItemStatus::Done);

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check
    assert_eq!(rows, 1, "a bare call is one row");
    assert!(
        row_trimmed(&backend, 0).contains("→ a.rs"),
        "the call line must render; got {:?}",
        row_trimmed(&backend, 0)
    );
    Ok(())
}

/// WHEN a tool call paints its line, THE name SHALL carry the body text style
/// and the summary the muted style — two distinguishable spans after the gutter.
#[test]
fn test_renderer_layout_tool_call_line_paints_name_and_muted_summary() -> Result<()> {
    // -- Setup & Fixtures
    let theme = test_theme();
    let entry = tool_entry("edit", r#"{"path":"foo.rs"}"#, None, None, ItemStatus::Done);

    // -- Exec
    let backend = render_at(&entry, 0)?.0;

    // -- Check: the gutter is four columns, then the name, then a space and the
    // summary.
    let row = row_text(&backend, 0);
    assert!(
        row.starts_with("  \u{2699} \u{2713} edit → foo.rs (diff)"),
        "got {row:?}"
    );
    assert_eq!(
        cell_fg(&backend, 6, 0),
        Some(terminal(theme.text)),
        "the tool name paints the body text style"
    );
    assert_eq!(
        cell_fg(&backend, 11, 0),
        Some(terminal(theme.muted)),
        "the summary paints the muted style"
    );
    assert_ne!(
        cell_fg(&backend, 6, 0),
        cell_fg(&backend, 11, 0),
        "name and summary must not share one style"
    );
    Ok(())
}

/// WHEN a tool call has no name, THE widget SHALL paint the summary alone — no
/// empty emphasis span and no gap after the indicator.
#[test]
fn test_renderer_layout_nameless_tool_paints_summary_only() -> Result<()> {
    // -- Setup & Fixtures
    let theme = test_theme();
    let entry = tool_entry("", r#"{"path":"foo.rs"}"#, None, None, ItemStatus::Done);

    // -- Exec
    let backend = render_at(&entry, 0)?.0;

    // -- Check
    assert_eq!(
        row_trimmed(&backend, 0),
        "  \u{2699} \u{2713} → {\"path\":\"foo.rs\"}",
        "the call line follows the indicator directly"
    );
    assert_eq!(
        cell_fg(&backend, 6, 0),
        Some(terminal(theme.muted)),
        "the summary keeps the muted style"
    );
    Ok(())
}

/// WHEN an in-progress tool call renders, THE widget SHALL paint content (the
/// gutter, the spinner and the summary).
#[test]
fn test_renderer_layout_tool_in_progress_paints_content() -> Result<()> {
    // -- Setup & Fixtures
    let entry = tool_entry(
        "read",
        r#"{"path":"x"}"#,
        None,
        None,
        ItemStatus::InProgress,
    );

    // -- Exec
    let backend = render(&entry)?;

    // -- Check
    let row = row_trimmed(&backend, 0);
    assert!(
        row.contains("\u{280b}") && row.contains("→ x"),
        "the in-progress call must paint the spinner and the summary; got {row:?}"
    );
    Ok(())
}

// endregion: --- Tool call line

/// WHEN a task line and a tool line carry the same amount of text, THEY SHALL
/// wrap at the same points: the task lane reserves the status indicator exactly
/// as the tool lane does.
#[test]
fn test_renderer_layout_task_wraps_like_the_tool_line() -> Result<()> {
    // -- Setup & Fixtures: space-free fillers, so the wrap is a character count.
    // The task's content leads with "Compaction · " (13 columns) where the tool's
    // leads with "read " (5), so each carries the filler that makes both total
    // lengths one column past two budgets.
    const TASK_LEAD: usize = 13;
    const TOOL_LEAD: usize = 5;
    let budget = content_wrap_width(
        usize::from(WIDTH),
        true,
        LaneKind::Compaction.prefix_width(),
    );
    let total = 2 * budget + 1;
    let task = task_entry(&"x".repeat(total - TASK_LEAD), ItemStatus::Done);
    let tool = tool_entry(
        "read",
        &"x".repeat(total - TOOL_LEAD),
        None,
        None,
        ItemStatus::Done,
    );

    // -- Exec
    let task_rows = measured_rows(&task)?;
    let tool_rows = measured_rows(&tool)?;

    // -- Check: one budget, one wrap; and one column past two budgets needs
    // three rows on both — a two-column-wider task budget would need two.
    assert_eq!(
        task_rows, tool_rows,
        "the task lane wraps like the tool lane"
    );
    assert_eq!(task_rows, 3, "two budgets plus one column needs three rows");
    Ok(())
}

// region:    --- Tasks, notices and banners

/// WHEN a task line renders, THE widget SHALL paint one call-line row.
#[test]
fn test_renderer_layout_task_paints_one_row() -> Result<()> {
    // -- Setup & Fixtures
    let entry = task_entry("auto", ItemStatus::Done);

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check
    assert_eq!(rows, 1);
    assert!(
        row_trimmed(&backend, 0).contains("Compaction · auto"),
        "got {:?}",
        row_trimmed(&backend, 0)
    );
    Ok(())
}

/// WHEN a banner renders, THE widget SHALL paint one row per source line.
#[test]
fn test_renderer_layout_banner_paints_every_line() -> Result<()> {
    // -- Setup & Fixtures
    let entry = banner_entry("line1\nline2");

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check
    assert_eq!(rows, 2);
    let text: String = (0..2).map(|y| row_text(&backend, y)).collect();
    assert!(
        text.contains("line1") && text.contains("line2"),
        "got {text:?}"
    );
    Ok(())
}

// endregion: --- Notices and banners
