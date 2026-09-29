use super::transcript::full_width_background;
use nu_agent_core::protocol::tool_args::CallLine;
use nu_agent_core::transcript::ir::{BlockSource, Fill, Lane, MessageRole, ToolName};
use nu_agent_core::transcript::renderer::FrameContext;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Theme colors used by `full_width_background`; values are opaque — tests
/// assert identity, not specific colors.
const FULL_BG: ratatui::style::Color = ratatui::style::Color::Indexed(16);
const CODE_BG: ratatui::style::Color = ratatui::style::Color::Indexed(17);

fn user_block() -> nu_agent_core::transcript::ir::Block {
    nu_agent_core::transcript::ir::Block {
        source: BlockSource::Markdown {
            role: MessageRole::User,
            markdown: "hi".to_string(),
        },
        lane: Lane::Marker("▏"),
        fill: Fill::Full,
        status: None,
    }
}

fn assistant_block() -> nu_agent_core::transcript::ir::Block {
    nu_agent_core::transcript::ir::Block {
        source: BlockSource::Markdown {
            role: MessageRole::Assistant,
            markdown: "hi".to_string(),
        },
        lane: Lane::Blank,
        fill: Fill::None,
        status: None,
    }
}

fn spacer_block() -> nu_agent_core::transcript::ir::Block {
    nu_agent_core::transcript::ir::Block {
        source: BlockSource::Spacer,
        lane: Lane::Blank,
        fill: Fill::None,
        status: None,
    }
}

fn tool_block(fill: Fill) -> nu_agent_core::transcript::ir::Block {
    nu_agent_core::transcript::ir::Block {
        source: BlockSource::Tool {
            name: ToolName("tool".to_string()),
            call: CallLine {
                summary: "→ x".to_string(),
            },
            preview: None,
        },
        lane: Lane::Marker("⚙"),
        fill,
        status: None,
    }
}

#[test]
fn user_block_needs_user_bg() {
    let blocks = vec![user_block()];
    assert_eq!(
        full_width_background(&blocks, 0, FULL_BG, CODE_BG),
        Some(FULL_BG)
    );
}

#[test]
fn assistant_block_does_not_need_user_bg() {
    let blocks = vec![assistant_block()];
    assert_eq!(full_width_background(&blocks, 0, FULL_BG, CODE_BG), None);
}

#[test]
fn spacer_after_user_needs_user_bg() {
    let blocks = vec![user_block(), spacer_block()];
    assert_eq!(
        full_width_background(&blocks, 1, FULL_BG, CODE_BG),
        Some(FULL_BG)
    );
}

#[test]
fn spacer_before_user_needs_user_bg() {
    let blocks = vec![spacer_block(), user_block()];
    assert_eq!(
        full_width_background(&blocks, 0, FULL_BG, CODE_BG),
        Some(FULL_BG)
    );
}

#[test]
fn spacer_between_two_users_needs_user_bg() {
    let blocks = vec![user_block(), spacer_block(), user_block()];
    assert_eq!(
        full_width_background(&blocks, 1, FULL_BG, CODE_BG),
        Some(FULL_BG)
    );
}

#[test]
fn spacer_not_adjacent_to_user_does_not_need_user_bg() {
    let blocks = vec![assistant_block(), spacer_block(), assistant_block()];
    assert_eq!(full_width_background(&blocks, 1, FULL_BG, CODE_BG), None);
}

#[test]
fn out_of_range_block_does_not_need_user_bg() {
    let blocks = vec![user_block()];
    assert_eq!(full_width_background(&blocks, 5, FULL_BG, CODE_BG), None);
}

#[test]
fn tool_block_does_not_need_user_bg() {
    let blocks = vec![tool_block(Fill::None)];
    assert_eq!(full_width_background(&blocks, 0, FULL_BG, CODE_BG), None);
}

// ── full_width_background: Fill::Code ───────────────────────────────────────

#[test]
fn fill_code_block_needs_code_bg() {
    let blocks = vec![tool_block(Fill::Code)];
    assert_eq!(
        full_width_background(&blocks, 0, FULL_BG, CODE_BG),
        Some(CODE_BG)
    );
}

#[test]
fn fill_none_block_does_not_need_code_bg() {
    let blocks = vec![tool_block(Fill::None)];
    assert_eq!(full_width_background(&blocks, 0, FULL_BG, CODE_BG), None);
}

#[test]
fn out_of_range_block_does_not_need_code_bg() {
    let blocks = vec![tool_block(Fill::Code)];
    assert_eq!(full_width_background(&blocks, 5, FULL_BG, CODE_BG), None);
}

// ── row-count parity (margin rows) ──────────────────────────────────────────

// ── layout() row accounting (Fill::Code margin rows) ───────────────────────

fn nu_tool_block(code: &str) -> nu_agent_core::transcript::ir::Block {
    nu_agent_core::transcript::ir::Block {
        source: BlockSource::Tool {
            name: ToolName("nu".to_string()),
            call: CallLine {
                summary: code.to_string(),
            },
            preview: Some(nu_agent_core::transcript::ir::Display {
                title: "nu".to_string(),
                sections: vec![nu_agent_core::transcript::ir::DisplaySection {
                    label: "nu".to_string(),
                    kind: nu_agent_core::transcript::ir::ContentKind::Code {
                        language: "nu".to_string(),
                    },
                    content: code.to_string(),
                    stats: None,
                }],
            }),
        },
        lane: Lane::Marker("⚙"),
        fill: Fill::Code,
        status: None,
    }
}

#[test]
fn fill_code_layout_includes_margin_rows_matching_accounting() -> Result<()> {
    use crate::state::TranscriptStore;
    use crate::tui_renderer::layout;

    let block = nu_tool_block("ls | where size > 1mb\n| select name type\n| sort-by modified");
    for width in [80usize, 120usize] {
        let rendered = layout(&block, &frame_ctx(width)).len();

        // -- Exec: run the same accounting the render loop uses
        let mut store = TranscriptStore::default();
        store.push_block(block.clone());
        store.rebuild_height_index(width);

        // -- Check: accounting includes the 2 margin rows layout() injects
        assert_eq!(
            store.total_visual_rows(),
            rendered,
            "width {width}: visual accounting must match layout() row count incl. margins"
        );
    }
    Ok(())
}

fn frame_ctx(width: usize) -> FrameContext {
    FrameContext {
        width,
        now_millis: 0,
        cursor: false,
        selected: false,
    }
}

#[test]
fn status_indicator_wraps_within_pane_width() -> Result<()> {
    use crate::tui_renderer::layout;

    // A tool whose row-0 summary wraps at the pane width. With a status
    // indicator on row 0, the renderer must shrink the wrap budget by 2
    // columns so row 0 fits the pane and ratatui does not re-wrap it into an
    // extra visual row.
    let block = nu_agent_core::transcript::ir::Block {
        source: BlockSource::Tool {
            name: ToolName("tool".to_string()),
            call: CallLine {
                summary: format!("\u{2192} {}", "x".repeat(200)),
            },
            preview: None,
        },
        lane: Lane::Marker("⚙"),
        fill: Fill::None,
        status: Some(nu_agent_core::transcript::renderer::ItemStatus::InProgress),
    };
    for width in [80usize, 120usize] {
        let lines = layout(&block, &frame_ctx(width));
        for line in &lines {
            let line_width: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
            assert!(
                line_width <= width,
                "width {width}: rendered line {line_width} exceeds pane width {width}"
            );
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Render loop end-to-end (task 7a671267)
// ---------------------------------------------------------------------------

use crate::runtime::coordinator::RuntimeCoordinator;
use nu_agent_core::transcript::ir::Block;
use nu_agent_core::transcript::ir::Lane as L;
use ratatui::layout::Rect;

/// Render the transcript pane through `render_transcript_pane` onto a
/// `TestBackend` and return the terminal for buffer inspection. Mirrors the
/// production coordinator layout: the list area equals the content area with
/// the width inset by 2 (1 margin column each side).
fn render_transcript_pane_to_backend(
    coord: &mut RuntimeCoordinator,
    columns: u16,
    rows: u16,
    following_tail: bool,
    scroll_offset: usize,
) -> ratatui::Terminal<ratatui::backend::TestBackend> {
    let backend = ratatui::backend::TestBackend::new(columns, rows);
    let mut terminal = match ratatui::Terminal::new(backend) {
        Ok(terminal) => terminal,
        // TestBackend terminal construction is infallible: the error type is
        // uninhabited.
        Err(never) => match never {},
    };
    let content = Rect {
        x: 0,
        y: 0,
        width: columns,
        height: rows,
    };
    let list = Rect {
        width: columns.saturating_sub(2),
        ..content
    };
    let draw_result = terminal.draw(|frame| {
        let mut rendered_scroll_offset = None;
        coord.render_transcript_pane(
            frame,
            content,
            list,
            following_tail,
            scroll_offset,
            &mut rendered_scroll_offset,
        );
    });
    // TestBackend draw is infallible; satisfy the no-unwrap policy without a
    // match on an uninhabited error type.
    if let Err(never) = draw_result {
        match never {}
    }
    terminal
}

/// Background of the cell at `(x, y)` in the rendered buffer.
fn cell_bg(
    terminal: &ratatui::Terminal<ratatui::backend::TestBackend>,
    x: u16,
    y: u16,
) -> Option<ratatui::style::Color> {
    terminal.backend().buffer()[(x, y)].style().bg
}

#[test]
fn render_loop_full_fill_user_rows_and_adjacent_separator_bleed_user_bg() -> Result<()> {
    // -- Setup & Fixtures
    let mut coord = RuntimeCoordinator::new(40, 20, None);
    let columns = 40u16;
    let rows = 20u16;
    let viewport = rows as usize;
    // Three content blocks: banner, assistant prose, user prompt. SM separators:
    //   banner → assistant = 2; assistant → user = 2. Total visual rows:
    //   1 (banner) + 2 + 1 (assistant) + 2 + 1 (user) = 7.
    coord.state.transcript.push_block(Block {
        source: BlockSource::Banner {
            text: "N".to_string(),
        },
        lane: L::SystemBlank,
        fill: Fill::None,
        status: None,
    });
    coord.state.transcript.push_block(Block {
        source: BlockSource::Markdown {
            role: MessageRole::Assistant,
            markdown: "assistant reply".to_string(),
        },
        lane: L::Blank,
        fill: Fill::None,
        status: None,
    });
    coord.state.transcript.push_block(Block {
        source: BlockSource::Markdown {
            role: MessageRole::User,
            markdown: "hi".to_string(),
        },
        lane: L::Marker("▏"),
        fill: Fill::Full,
        status: None,
    });

    // Layout rows: [banner(0)] [sep(1,2)] [assistant(3)] [sep(4,5)] [user(6)].
    // Content (7 rows) is shorter than the 20-row viewport, so it bottom-aligns
    // with padding/2 = 6 rows above. Absolute rows: banner 6, separators 7-8,
    // assistant 9, separators 10-11, user 12.
    let padding = viewport - 7;
    let top_padding = padding / 2;
    let banner_row = top_padding;
    let user_row = top_padding + 6;
    let separator_after_assistant = top_padding + 5;

    // -- Exec
    let terminal = render_transcript_pane_to_backend(&mut coord, columns, rows, true, 0);

    // -- Check
    let full_bg = coord.theme.row_user_bg;
    let base = coord.theme.base;
    // Every pane cell is painted (no transparent leftovers).
    for y in 0..rows {
        for x in 1..(columns - 1) {
            let bg =
                cell_bg(&terminal, x, y).ok_or("every transcript cell must have a background")?;
            assert!(
                bg == full_bg || bg == base,
                "cell ({x},{y}) bg {bg:?} must be user bg or base"
            );
        }
    }
    // The Fill::Full user row paints the user background.
    assert_eq!(
        cell_bg(&terminal, 1, user_row as u16),
        Some(full_bg),
        "user row must paint the user background"
    );
    // The separator row directly below the user row bleeds the user
    // background (the pre-refactor bleed, preserved across the render-time
    // separator rows).
    assert_eq!(
        cell_bg(&terminal, 1, user_row as u16),
        Some(full_bg),
        "user row background must be continuous"
    );
    // The separator row directly above the user block bleeds the user bg too.
    assert_eq!(
        cell_bg(&terminal, 1, separator_after_assistant as u16),
        Some(full_bg),
        "separator row adjacent to the incoming user block must bleed user bg"
    );
    // Non-user content stays on the base background.
    assert_eq!(
        cell_bg(&terminal, 1, banner_row as u16),
        Some(base),
        "banner row must stay on the base background"
    );
    Ok(())
}

#[test]
fn render_loop_visible_window_follows_visual_scroll_offset() -> Result<()> {
    // -- Setup & Fixtures
    let mut coord = RuntimeCoordinator::new(40, 20, None);
    for i in 1..=6 {
        coord.state.transcript.push_block(Block {
            source: BlockSource::Markdown {
                role: MessageRole::Assistant,
                markdown: format!("assistant text {i}"),
            },
            lane: L::Blank,
            fill: Fill::None,
            status: None,
        });
    }
    // Six assistant blocks: the first has no separator, each later block
    // carries 2 (assistant → assistant), so each block spans:
    //   A1 rows 0; A2 rows 1..=3; A3 rows 4..=6; A4 rows 7..=9;
    //   A5 rows 10..=12; A6 rows 13..=15. Total 16 visual rows.
    coord.state.scroll.following_tail = false;
    coord.state.scroll.scroll_offset = 3;

    // -- Exec
    let terminal = render_transcript_pane_to_backend(&mut coord, 40, 4, false, 3);

    // -- Check
    // Offset 3 with viewport 4 covers visual rows 3..7: A2's content row (3),
    // A3's two separator rows (4,5), and A3's content row (6). The visible
    // block window is (1, 3); the renderer emits A2's separators too, so the
    // emitted lines map to block indices [1,1,1,2,2,2].
    let indices = &coord.state.scroll.entry_indices;
    assert_eq!(indices.len(), 6, "one index per emitted line");
    assert_eq!(
        indices.as_slice(),
        [1usize, 1, 1, 2, 2, 2],
        "entry indices must map emitted lines to block indices"
    );
    let text: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(
        text.contains("assistant text 2"),
        "block A2 must be visible"
    );
    assert!(
        text.contains("assistant text 3"),
        "block A3 must be visible"
    );
    assert!(
        !text.contains("assistant text 1"),
        "blocks above the offset must not render"
    );
    assert!(
        !text.contains("assistant text 4")
            && !text.contains("assistant text 5")
            && !text.contains("assistant text 6"),
        "blocks below the viewport must not render"
    );
    Ok(())
}

#[test]
fn render_loop_renders_tool_blocks_via_layout_with_accounting() -> Result<()> {
    // -- Setup & Fixtures
    let mut coord = RuntimeCoordinator::new(40, 20, None);
    coord.state.transcript.push_block(Block {
        source: BlockSource::Tool {
            name: ToolName("tool".to_string()),
            call: CallLine {
                summary: "→ alpha".to_string(),
            },
            preview: None,
        },
        lane: L::Marker("⚙"),
        fill: Fill::None,
        status: None,
    });
    coord.state.transcript.push_block(Block {
        source: BlockSource::Tool {
            name: ToolName("tool".to_string()),
            call: CallLine {
                summary: "→ beta".to_string(),
            },
            preview: None,
        },
        lane: L::Marker("⚙"),
        fill: Fill::None,
        status: None,
    });

    // -- Exec
    let terminal = render_transcript_pane_to_backend(&mut coord, 40, 6, true, 0);

    // -- Check
    // Both tool blocks render through layout() — their call summaries appear.
    let text: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(
        text.contains("→ alpha"),
        "tool block 1 must render, got:\n{text}"
    );
    assert!(
        text.contains("→ beta"),
        "tool block 2 must render, got:\n{text}"
    );
    // Two tool blocks with no previews: the SM returns 0 separators between
    // them, so they render as 2 adjacent visual rows, bottom-aligned in the
    // 6-row viewport (padding 4 → 2 rows above, 2 below).
    assert_eq!(
        coord.state.scroll.entry_indices,
        [0usize, 0, 0, 1, 0, 0],
        "2 tool rows padded to the viewport with padding rows mapped to block 0"
    );
    // The measure()-based height index drives scroll math: 2 tool rows, no
    // separator rows (tool → tool).
    assert_eq!(coord.state.scroll.total_visual_rows, 2);
    assert_eq!(coord.state.scroll.max_scroll, 0);
    Ok(())
}

// ---------------------------------------------------------------------------
// Two-block preview shape: call line untinted, preview-only fill (task 4b70df9b)
// ---------------------------------------------------------------------------

/// WHEN the render loop renders a Tool block with `Fill::None` followed by a
/// ToolDisplay block with `Fill::Code`, THE renderer SHALL paint the call line
/// row untinted with no margin row above it, and SHALL paint surface0 with
/// margin rows only around the preview content rows.
#[test]
fn render_loop_tool_then_preview_block_leaves_call_line_untinted() -> Result<()> {
    // -- Setup & Fixtures: the production two-block shape — the Tool block
    // carries no preview and no fill; the preview is its own ToolDisplay block
    // with Fill::Code.
    let mut coord = RuntimeCoordinator::new(40, 8, None);
    let columns = 40u16;
    let rows = 8u16;
    coord.state.transcript.push_block(Block {
        source: BlockSource::Tool {
            name: ToolName("nu".to_string()),
            call: CallLine {
                summary: "ls | select name".to_string(),
            },
            preview: None,
        },
        lane: L::Marker("⚙"),
        fill: Fill::None,
        status: None,
    });
    coord.state.transcript.push_block(Block {
        source: BlockSource::ToolDisplay {
            lines: nu_agent_core::transcript::markdown::project_code_block_lines(
                "nu",
                "ls | select name",
            ),
        },
        lane: L::Blank,
        fill: Fill::Code,
        status: None,
    });

    // -- Exec
    let terminal = render_transcript_pane_to_backend(&mut coord, columns, rows, true, 0);

    // -- Check: 1 call-line row + 3 preview rows (top margin, content, bottom
    // margin) = 4 visual rows, bottom-aligned in the 8-row viewport with 2
    // padding rows above and 2 below.
    assert_eq!(
        coord.state.scroll.total_visual_rows, 4,
        "call line + preview margins and content"
    );
    let surface0 = coord.theme.surface0;
    let base = coord.theme.base;
    let call_line_row = 2u16;
    let top_margin_row = 3u16;
    let content_row = 4u16;
    let bottom_margin_row = 5u16;

    // The call line stays untinted, and no margin row sits above it.
    assert_ne!(
        cell_bg(&terminal, 1, call_line_row),
        Some(surface0),
        "the call line row must not carry the code surface"
    );
    assert_eq!(
        cell_bg(&terminal, 1, call_line_row - 1),
        Some(base),
        "no margin row may sit above the call line"
    );

    // The preview block owns the surface0 fill and the margin rows.
    for (label, y) in [
        ("top margin", top_margin_row),
        ("content", content_row),
        ("bottom margin", bottom_margin_row),
    ] {
        assert_eq!(
            cell_bg(&terminal, 1, y),
            Some(surface0),
            "preview {label} row must carry surface0"
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Filled preview block separates the next tool call (task 59bcffc6)
// ---------------------------------------------------------------------------

/// WHEN the render loop lays out a Tool block, a filled ToolDisplay preview
/// block, and a following Tool block, THE render loop SHALL emit one separator
/// row between the filled preview and the next call line, so the code surface
/// does not sit flush against the following tool row.
#[test]
fn render_loop_filled_preview_block_separates_next_tool_call() -> Result<()> {
    // -- Setup & Fixtures: the production shape — the nu call line, its
    // Fill::Code preview block, then a second tool call.
    let mut coord = RuntimeCoordinator::new(40, 8, None);
    let columns = 40u16;
    let rows = 8u16;
    coord.state.transcript.push_block(Block {
        source: BlockSource::Tool {
            name: ToolName("nu".to_string()),
            call: CallLine {
                summary: String::new(),
            },
            preview: None,
        },
        lane: L::Marker("⚙"),
        fill: Fill::None,
        status: None,
    });
    coord.state.transcript.push_block(Block {
        source: BlockSource::ToolDisplay {
            lines: nu_agent_core::transcript::markdown::project_code_block_lines(
                "nu",
                "ls | select name",
            ),
        },
        lane: L::Blank,
        fill: Fill::Code,
        status: None,
    });
    coord.state.transcript.push_block(Block {
        source: BlockSource::Tool {
            name: ToolName("read".to_string()),
            call: CallLine {
                summary: "→ a.rs".to_string(),
            },
            preview: None,
        },
        lane: L::Marker("⚙"),
        fill: Fill::None,
        status: None,
    });

    // -- Exec
    let terminal = render_transcript_pane_to_backend(&mut coord, columns, rows, true, 0);

    // -- Check: 1 nu call line + 3 preview rows (top margin, content, bottom
    // margin) + 1 separator + 1 read call line = 6 visual rows. The 8-row
    // viewport leaves 2 padding rows, split evenly: 1 above (row 0) and 1
    // below (row 7). The nu call line is therefore row 1, the preview occupies
    // rows 2-4, the separator is row 5, and the read call line is row 6.
    assert_eq!(
        coord.state.scroll.total_visual_rows, 6,
        "call line + preview + separator + next call line"
    );
    let surface0 = coord.theme.surface0;
    let base = coord.theme.base;
    let separator_row = 5u16;
    let read_call_line_row = 6u16;

    assert_eq!(
        cell_bg(&terminal, 1, separator_row),
        Some(base),
        "the separator row between the filled preview and the next tool paints the base background"
    );
    assert_ne!(
        cell_bg(&terminal, 1, read_call_line_row),
        Some(surface0),
        "the read call line must not carry the code surface"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Trailing separator after a final user block (task 670e0292)
// ---------------------------------------------------------------------------

#[test]
fn render_loop_trailing_separator_after_final_user_block() -> Result<()> {
    // -- Setup & Fixtures: one user block, and it is the final block.
    let mut coord = RuntimeCoordinator::new(40, 2, None);
    let columns = 40u16;
    let rows = 2u16;
    coord.state.transcript.push_block(user_block());

    // -- Exec
    let terminal = render_transcript_pane_to_backend(&mut coord, columns, rows, true, 0);

    // -- Check: 1 content row + 1 trailing separator row = 2 visual rows.
    assert_eq!(
        coord.state.scroll.total_visual_rows, 2,
        "a final user block adds one trailing separator row to the total"
    );
    assert_eq!(
        coord.state.scroll.entry_indices,
        [0usize, 0],
        "the trailing separator row maps to the user block"
    );
    let full_bg = coord.theme.row_user_bg;
    assert_eq!(
        cell_bg(&terminal, 1, 0),
        Some(full_bg),
        "the user content row paints the user background"
    );
    assert_eq!(
        cell_bg(&terminal, 1, 1),
        Some(full_bg),
        "the trailing separator row below the user block bleeds the user background"
    );
    Ok(())
}

#[test]
fn render_loop_no_trailing_separator_after_final_assistant_block() -> Result<()> {
    // -- Setup & Fixtures: one assistant block as the final block.
    let mut coord = RuntimeCoordinator::new(40, 1, None);
    coord.state.transcript.push_block(assistant_block());

    // -- Exec
    let terminal = render_transcript_pane_to_backend(&mut coord, 40, 1, true, 0);

    // -- Check: no trailing row — the total is the single content row.
    assert_eq!(
        coord.state.scroll.total_visual_rows, 1,
        "a non-user final block adds no trailing separator row"
    );
    assert_eq!(
        coord.state.scroll.entry_indices,
        [0usize],
        "only the assistant content row is rendered"
    );
    let base = coord.theme.base;
    assert_eq!(
        cell_bg(&terminal, 1, 0),
        Some(base),
        "the assistant row stays on the base background"
    );
    Ok(())
}

#[test]
fn render_loop_user_followed_by_block_has_no_trailing_double_count() -> Result<()> {
    // -- Setup & Fixtures: a user block followed by an assistant block, so the
    // user block is NOT the final block. Gap between the two blocks is exactly
    // 2 separator rows (user → assistant); no trailing row is added after the
    // user block itself.
    let mut coord = RuntimeCoordinator::new(40, 20, None);
    coord.state.transcript.push_block(user_block());
    coord.state.transcript.push_block(assistant_block());

    // -- Exec
    render_transcript_pane_to_backend(&mut coord, 40, 20, true, 0);

    // -- Check: 1 (user) + 2 (separators) + 1 (assistant) = 4 rows; the
    // final block is assistant, which adds no trailing row.
    assert_eq!(
        coord.state.scroll.total_visual_rows, 4,
        "the gap between user and assistant is 2 rows with no extra trailing row"
    );
    Ok(())
}
