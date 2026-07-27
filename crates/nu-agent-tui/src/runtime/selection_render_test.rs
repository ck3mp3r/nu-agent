use super::RuntimeCoordinator;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

/// Create a test buffer with distinct foreground colors per cell so we can
/// verify that the highlight style does NOT overwrite them.
fn make_test_buffer(width: u16, height: u16) -> Buffer {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    for y in 0..height {
        for x in 0..width {
            let cell = buf.cell_mut((x, y)).expect("cell in range");
            cell.set_symbol(" ");
            cell.set_style(
                Style::default()
                    .fg(Color::Indexed(((y * width + x) % 256) as u8))
                    .bg(Color::Black),
            );
        }
    }
    buf
}

/// Assert that every cell in the given row range has the highlight style applied.
fn assert_row_highlighted(buf: &Buffer, area: Rect, row_y: u16) {
    for x in area.x..area.x + area.width {
        let cell = buf.cell((x, row_y)).expect("cell in range");
        assert_eq!(
            cell.bg,
            Color::DarkGray,
            "row {row_y} col {x}: expected DarkGray background"
        );
        assert!(
            cell.modifier.contains(Modifier::REVERSED),
            "row {row_y} col {x}: expected REVERSED modifier"
        );
    }
}

/// Assert that every cell in the given row range has the default (non-highlight) style.
fn assert_row_not_highlighted(buf: &Buffer, area: Rect, row_y: u16) {
    for x in area.x..area.x + area.width {
        let cell = buf.cell((x, row_y)).expect("cell in range");
        assert_ne!(
            cell.bg,
            Color::DarkGray,
            "row {row_y} col {x}: should not have DarkGray background"
        );
        assert!(
            !cell.modifier.contains(Modifier::REVERSED),
            "row {row_y} col {x}: should not have REVERSED modifier"
        );
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn apply_selection_highlight_highlights_selected_rows() {
    let area = Rect::new(0, 0, 5, 5);
    let mut buf = make_test_buffer(5, 5);

    RuntimeCoordinator::apply_selection_highlight(
        &mut buf, area, 1, // sel_start
        3, // sel_end
        0, // effective_offset
        5, // viewport_height
    );

    // Rows 0 and 4 should NOT be highlighted
    assert_row_not_highlighted(&buf, area, 0);
    assert_row_not_highlighted(&buf, area, 4);

    // Rows 1, 2, 3 should be highlighted
    assert_row_highlighted(&buf, area, 1);
    assert_row_highlighted(&buf, area, 2);
    assert_row_highlighted(&buf, area, 3);
}

#[test]
fn apply_selection_highlight_skips_rows_above_viewport() {
    let area = Rect::new(0, 0, 5, 5);
    let mut buf = make_test_buffer(5, 5);

    // Selection starts at row 0 but viewport starts at offset 3 — nothing visible
    RuntimeCoordinator::apply_selection_highlight(
        &mut buf, area, 0, // sel_start
        1, // sel_end
        3, // effective_offset
        5, // viewport_height
    );

    // No rows should be highlighted
    for y in 0..5 {
        assert_row_not_highlighted(&buf, area, y);
    }
}

#[test]
fn apply_selection_highlight_skips_rows_below_viewport() {
    let area = Rect::new(0, 0, 5, 5);
    let mut buf = make_test_buffer(5, 5);

    // Selection is at rows 5-6, viewport shows rows 0-4 — nothing visible
    RuntimeCoordinator::apply_selection_highlight(
        &mut buf, area, 5, // sel_start
        6, // sel_end
        0, // effective_offset
        5, // viewport_height
    );

    for y in 0..5 {
        assert_row_not_highlighted(&buf, area, y);
    }
}

#[test]
fn apply_selection_highlight_single_row() {
    let area = Rect::new(0, 0, 5, 5);
    let mut buf = make_test_buffer(5, 5);

    RuntimeCoordinator::apply_selection_highlight(
        &mut buf, area, 2, // sel_start
        2, // sel_end
        0, // effective_offset
        5, // viewport_height
    );

    // Only row 2 should be highlighted
    assert_row_not_highlighted(&buf, area, 0);
    assert_row_not_highlighted(&buf, area, 1);
    assert_row_highlighted(&buf, area, 2);
    assert_row_not_highlighted(&buf, area, 3);
    assert_row_not_highlighted(&buf, area, 4);
}

#[test]
fn apply_selection_highlight_preserves_non_highlighted_cells() {
    let area = Rect::new(0, 0, 5, 5);
    let mut buf = make_test_buffer(5, 5);

    // Snapshot original styles for rows 0 and 4
    let orig_row0: Vec<_> = (0..area.width)
        .map(|x| {
            let c = buf.cell((x, 0)).expect("cell");
            (c.fg, c.bg, c.modifier)
        })
        .collect();
    let orig_row4: Vec<_> = (0..area.width)
        .map(|x| {
            let c = buf.cell((x, 4)).expect("cell");
            (c.fg, c.bg, c.modifier)
        })
        .collect();

    RuntimeCoordinator::apply_selection_highlight(
        &mut buf, area, 1, // sel_start
        3, // sel_end
        0, // effective_offset
        5, // viewport_height
    );

    // Row 0 should be completely unchanged
    for (x, &(fg, bg, modifier)) in orig_row0.iter().enumerate() {
        let cell = buf.cell((x as u16, 0)).expect("cell");
        assert_eq!(
            cell.fg, fg,
            "row 0 col {x}: foreground color was overwritten"
        );
        assert_eq!(
            cell.bg, bg,
            "row 0 col {x}: background color was overwritten"
        );
        assert_eq!(
            cell.modifier, modifier,
            "row 0 col {x}: modifier was overwritten"
        );
    }

    // Row 4 should be completely unchanged
    for (x, &(fg, bg, modifier)) in orig_row4.iter().enumerate() {
        let cell = buf.cell((x as u16, 4)).expect("cell");
        assert_eq!(
            cell.fg, fg,
            "row 4 col {x}: foreground color was overwritten"
        );
        assert_eq!(
            cell.bg, bg,
            "row 4 col {x}: background color was overwritten"
        );
        assert_eq!(
            cell.modifier, modifier,
            "row 4 col {x}: modifier was overwritten"
        );
    }
}
