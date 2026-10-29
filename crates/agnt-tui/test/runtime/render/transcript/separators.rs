use super::*;

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
    assert_eq!(
        cell_symbol(&terminal, 0, 1),
        "╎",
        "the trailing separator row below the user block must carry the user rail"
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
