use super::*;

#[test]
fn render_loop_full_fill_user_rows_and_adjacent_separator_bleed_user_bg() -> Result<()> {
    // -- Setup & Fixtures
    let mut coord = RuntimeCoordinator::new(40, 20, None);
    let columns = 40u16;
    let rows = 20u16;
    let viewport = rows as usize;
    // Three content entries: banner, assistant prose, user prompt. The
    // assistant owns its (1,1) padding, so the pair gaps are the base table
    // minus those padding rows: banner → assistant = 2 - 1 = 1 (its top row is
    // clipped, the bottom kept) and assistant → user = 2 - 1 = 1. Total visual
    // rows: 1 (banner) + 1 + 3 (padding, content, padding) + 1 + 1 (user) = 7 —
    // the landed total.
    coord.state.transcript.push_entry(banner_entry("N"));
    coord
        .state
        .transcript
        .push_entry(assistant_entry("assistant reply"));
    coord.state.transcript.push_entry(user_entry("hi"));

    // Layout rows: [banner(0)] [gap(1)] [assistant pad(2), content(3), pad(4)]
    // [gap(5)] [user(6)]. Content (7 rows) is shorter than the 20-row viewport,
    // so it bottom-aligns with padding/2 = 6 rows above. Absolute rows: banner
    // 6, gap 7, assistant 8-10, gap 11, user 12.
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
    // The separator row directly above the user block bleeds the user bg too,
    // and carries the user rail at list-local 0.
    assert_eq!(
        cell_bg(&terminal, 1, separator_after_assistant as u16),
        Some(full_bg),
        "separator row adjacent to the incoming user block must bleed user bg"
    );
    assert_eq!(
        cell_symbol(&terminal, 0, separator_after_assistant as u16),
        "╎",
        "the bleed row above the user block must carry the user rail"
    );
    // Non-user content stays on the base background.
    assert_eq!(
        cell_bg(&terminal, 1, banner_row as u16),
        Some(base),
        "banner row must stay on the base background"
    );
    Ok(())
}

/// WHEN the transcript renders a user block bracketed by non-user blocks, THE
/// separator rows directly above and below the block SHALL carry `╎` at
/// list-local 0 on the user background, WHILE every other separator row of each
/// run SHALL stay blank on the base background (task 4d99956c).
#[test]
fn render_loop_bleed_rows_above_and_below_user_block_carry_rail() -> Result<()> {
    // -- Setup & Fixtures: assistant, user, assistant. Each assistant keeps a
    // (1,1) padding, so each pair still spans two blank rows: the assistant's own
    // padding row plus one bleed separator row. The user box (one text row, one
    // bleed row above, one below) therefore keeps exactly one rail row on each
    // side, with the assistant's padding row beyond it.
    let mut coord = RuntimeCoordinator::new(40, 20, None);
    coord.state.transcript.push_entry(assistant_entry("above"));
    coord.state.transcript.push_entry(user_entry("rail-user"));
    coord.state.transcript.push_entry(assistant_entry("below"));

    // -- Exec
    let terminal = render_transcript_pane_to_backend(&mut coord, 40, 20, true, 0);

    // -- Check: 1 + 2 + 1 + 2 + 1 rows, bottom-aligned in the 20-row viewport.
    assert_eq!(
        coord.state.scroll.total_visual_rows, 7,
        "the fixture must render two 2-row separator runs"
    );
    let user_row = (0..20u16)
        .find(|y| {
            (0..40u16)
                .map(|x| cell_symbol(&terminal, x, *y))
                .collect::<String>()
                .contains("rail-user")
        })
        .ok_or("the user block must render")?;
    let full_bg = coord.theme.row_user_bg;
    let base = coord.theme.base;
    for (label, y) in [("above", user_row - 1), ("below", user_row + 1)] {
        assert_eq!(
            cell_symbol(&terminal, 0, y),
            "╎",
            "the bleed row {label} the user block must carry the user rail"
        );
        assert_eq!(
            cell_bg(&terminal, 0, y),
            Some(full_bg),
            "the bleed row {label} the user block must paint the user background"
        );
    }
    for (label, y) in [("above", user_row - 2), ("below", user_row + 2)] {
        assert_eq!(
            cell_symbol(&terminal, 0, y),
            " ",
            "the far separator row {label} the user block must stay blank"
        );
        assert_eq!(
            cell_bg(&terminal, 0, y),
            Some(base),
            "the far separator row {label} the user block must stay on base"
        );
    }
    Ok(())
}

#[test]
fn render_loop_visible_window_follows_visual_scroll_offset() -> Result<()> {
    // -- Setup & Fixtures
    let mut coord = RuntimeCoordinator::new(40, 20, None);
    for i in 1..=6 {
        coord
            .state
            .transcript
            .push_entry(assistant_entry(&format!("assistant text {i}")));
    }
    // Six assistant blocks. The frame has no outer margin and prose carries no
    // padding of its own: every block is one content row and every pair keeps
    // the assistant→assistant base spacing of two rows — 16 visual rows.
    //   A1 row 0; A2 row 3; A3 row 6; A4 row 9; A5 row 12; A6 row 15.
    coord.state.scroll.following_tail = false;
    coord.state.scroll.scroll_offset = 3;

    // -- Exec
    let terminal = render_transcript_pane_to_backend(&mut coord, 40, 4, false, 3);

    // -- Check
    // Offset 3 with viewport 4 covers visual rows 3..7: A2's content row, the
    // two separator rows, then A3's content row.
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
    coord
        .state
        .transcript
        .push_entry(tool_entry("tool", "→ alpha"));
    coord
        .state
        .transcript
        .push_entry(tool_entry("tool", "→ beta"));

    // -- Exec
    let terminal = render_transcript_pane_to_backend(&mut coord, 40, 6, true, 0);

    // -- Check
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
    // The measure()-based height index drives scroll math: 2 tool rows, no
    // separator rows (tool → tool).
    assert_eq!(coord.state.scroll.total_visual_rows, 2);
    assert_eq!(coord.state.scroll.max_scroll, 0);
    Ok(())
}
