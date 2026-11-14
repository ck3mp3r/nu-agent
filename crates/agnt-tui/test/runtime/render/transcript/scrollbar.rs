use super::*;

// ---------------------------------------------------------------------------
// Scroll bar glyph (task 7753f868)
// ---------------------------------------------------------------------------

#[test]
fn test_transcript_scrollbar_thumb_renders_heavy_line_glyph() -> Result<()> {
    // -- Setup & Fixtures: 21 assistant blocks are 3 × 21 − 2 = 61 visual rows,
    // so a 20-row viewport scrolls 41 rows and the bar renders. Parked at
    // offset 20 — the middle — so the rail shows above and below the thumb.
    let mut coord = RuntimeCoordinator::new(40, 20, None);
    for i in 1..=21 {
        coord
            .state
            .transcript
            .push_entry(assistant_entry(&format!("assistant text {i}")));
    }
    let columns = 40u16;
    let rows = 20u16;
    let bar_x = columns - 1;

    // -- Exec
    let terminal = render_transcript_pane_to_backend(&mut coord, columns, rows, false, 20);

    // -- Check
    assert_eq!(
        coord.state.scroll.total_visual_rows, 61,
        "the fixture is 61 visual rows tall"
    );
    let focus = coord.theme.focus.fg;
    let rail = coord.theme.subtle_meta.fg;
    let base = coord.theme.base;

    // The expected bar column, top to bottom — one char per row:
    //   `T` thumb, glyph `┃`, foreground `theme.focus`
    //   `r` rail,  glyph `║`, foreground `theme.subtle_meta`
    // 7 rail rows, 7 thumb rows, 6 rail rows.
    let expected_picture = "rrrrrrrTTTTTTTrrrrrr";
    let expected: Vec<(String, Option<ratatui::style::Color>)> = expected_picture
        .chars()
        .map(|kind| match kind {
            'T' => ("┃".to_string(), focus),
            _ => ("║".to_string(), rail),
        })
        .collect();
    let actual: Vec<(String, Option<ratatui::style::Color>)> = (0..rows)
        .map(|y| {
            (
                cell_symbol(&terminal, bar_x, y),
                cell_fg(&terminal, bar_x, y),
            )
        })
        .collect();
    assert_eq!(
        actual, expected,
        "the bar column must be {expected_picture}"
    );
    for y in 0..rows {
        assert_eq!(
            cell_bg(&terminal, bar_x, y),
            Some(base),
            "the bar cell at row {y} keeps the pane base background"
        );
    }
    let pane_text: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(
        !pane_text.contains('█'),
        "the pane paints no solid block thumb"
    );
    Ok(())
}

#[test]
fn test_transcript_scrollbar_absent_without_overflow() -> Result<()> {
    // -- Setup & Fixtures: one assistant block, one visual row, a taller viewport.
    let mut coord = RuntimeCoordinator::new(40, 10, None);
    coord.state.transcript.push_entry(assistant_entry("hello"));
    let columns = 40u16;
    let rows = 10u16;

    // -- Exec
    let terminal = render_transcript_pane_to_backend(&mut coord, columns, rows, true, 0);

    // -- Check
    assert_eq!(
        coord.state.scroll.total_visual_rows, 1,
        "the fixture must fit the viewport"
    );
    // The expected rightmost column, top to bottom: no bar at all, so every row
    // stays the pane's blank base cell — blank glyph, reset foreground.
    let expected: Vec<(String, Option<ratatui::style::Color>)> = (0..rows)
        .map(|_| (" ".to_string(), Some(ratatui::style::Color::Reset)))
        .collect();
    let actual: Vec<(String, Option<ratatui::style::Color>)> = (0..rows)
        .map(|y| {
            (
                cell_symbol(&terminal, columns - 1, y),
                cell_fg(&terminal, columns - 1, y),
            )
        })
        .collect();
    assert_eq!(
        actual, expected,
        "no bar renders when the transcript fits the viewport"
    );
    Ok(())
}

// region:    --- Test Support

/// Foreground of the cell at `(x, y)` in the rendered buffer.
fn cell_fg(
    terminal: &ratatui::Terminal<ratatui::backend::TestBackend>,
    x: u16,
    y: u16,
) -> Option<ratatui::style::Color> {
    terminal.backend().buffer()[(x, y)].style().fg
}

// endregion: --- Test Support
