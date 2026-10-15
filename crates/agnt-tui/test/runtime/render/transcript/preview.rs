use super::*;

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
            lines: agnt_core::transcript::markdown::project_code_block_lines(
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
// Diff tint survives the render loop's full-width paint (task 669b4dac)
// ---------------------------------------------------------------------------

/// WHEN the render loop lays out a `Fill::Code` preview block whose lines carry
/// a `DiffTint::Add`, THE render loop SHALL paint the add tint across the row —
/// the diff tint SHALL win over the block's surface0 fill.
#[test]
fn render_loop_diff_tint_wins_over_code_surface_fill() -> Result<()> {
    // -- Setup & Fixtures: the production preview shape — a ToolDisplay block
    // with Fill::Code whose lines are the projected diff.
    let mut coord = RuntimeCoordinator::new(40, 8, None);
    let columns = 40u16;
    let rows = 8u16;
    let lines = agnt_core::transcript::markdown::project_diff_lines(
        "--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\n",
        "diff",
    );
    coord.state.transcript.push_block(Block {
        source: BlockSource::ToolDisplay { lines },
        lane: L::Blank,
        fill: Fill::Code,
        status: None,
    });

    // -- Exec
    let terminal = render_transcript_pane_to_backend(&mut coord, columns, rows, true, 0);

    // -- Check: find the row carrying the added body and assert its cell bg is
    // the add tint, not surface0.
    let add_bg = coord.theme.diff_add_bg;
    let surface0 = coord.theme.surface0;
    let mut checked = false;
    for y in 0..rows {
        let mut row_text = String::new();
        for x in 0..columns {
            row_text.push_str(terminal.backend().buffer()[(x, y)].symbol());
        }
        if row_text.contains("new") {
            checked = true;
            assert_eq!(
                cell_bg(&terminal, 1, y),
                Some(add_bg),
                "the added diff row must paint the add tint full-width, not surface0; row {y}: {row_text:?}"
            );
            assert_ne!(
                cell_bg(&terminal, 1, y),
                Some(surface0),
                "the code surface must not clobber the diff tint; row {y}: {row_text:?}"
            );
        }
    }
    assert!(
        checked,
        "the rendered buffer must contain the added diff body"
    );
    Ok(())
}

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
            lines: agnt_core::transcript::markdown::project_code_block_lines(
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
