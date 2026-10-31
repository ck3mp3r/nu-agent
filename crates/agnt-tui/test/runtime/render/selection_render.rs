use super::super::RuntimeCoordinator;
use crate::rendering::theme::TuiTheme;
use crate::runtime::render::transcript::PaintWindow;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// The selection style used in tests — mirrors the theme's `selection_bg`
/// (a background-only style, no REVERSED modifier).
const TEST_SELECTION_STYLE: Style = Style::new().bg(Color::Blue);

/// A paint window over a five-column buffer: absolute row `first_row` lands on
/// buffer row `top`, and `height` content rows fit below it.
fn paint_window<'a>(
    theme: &'a TuiTheme,
    first_row: usize,
    height: usize,
    top: usize,
) -> PaintWindow<'a> {
    PaintWindow {
        theme,
        width: 5,
        first_row,
        height,
        top,
    }
}

/// Create a test buffer with distinct foreground colors per cell so we can
/// verify that the highlight style does NOT overwrite them.
fn make_test_buffer(width: u16, height: u16) -> Result<Buffer> {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    for y in 0..height {
        for x in 0..width {
            let cell = buf.cell_mut((x, y)).ok_or("cell should be in range")?;
            cell.set_symbol(" ");
            cell.set_style(
                Style::default()
                    .fg(Color::Indexed(((y * width + x) % 256) as u8))
                    .bg(Color::Black),
            );
        }
    }
    Ok(buf)
}

/// Assert that every cell in the given row range has the highlight style applied.
fn assert_row_highlighted(buf: &Buffer, area: Rect, row_y: u16) -> Result<()> {
    for x in area.x..area.x + area.width {
        let cell = buf.cell((x, row_y)).ok_or("cell should be in range")?;
        assert_eq!(
            cell.bg,
            Color::Blue,
            "row {row_y} col {x}: expected selection background"
        );
    }
    Ok(())
}

/// Assert that every cell in the given row range has the default (non-highlight) style.
fn assert_row_not_highlighted(buf: &Buffer, area: Rect, row_y: u16) -> Result<()> {
    for x in area.x..area.x + area.width {
        let cell = buf.cell((x, row_y)).ok_or("cell should be in range")?;
        assert_ne!(
            cell.bg,
            Color::Blue,
            "row {row_y} col {x}: should not have selection background"
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn apply_selection_highlight_highlights_selected_rows() -> Result<()> {
    let area = Rect::new(0, 0, 5, 5);
    let mut buf = make_test_buffer(5, 5)?;

    let theme = TuiTheme::default();
    let window = paint_window(&theme, 0, 5, 0);
    RuntimeCoordinator::apply_selection_highlight(
        &mut buf,
        area,
        1, // sel_start
        3, // sel_end
        &window,
        TEST_SELECTION_STYLE,
    );

    // Rows 0 and 4 should NOT be highlighted
    assert_row_not_highlighted(&buf, area, 0)?;
    assert_row_not_highlighted(&buf, area, 4)?;

    // Rows 1, 2, 3 should be highlighted
    assert_row_highlighted(&buf, area, 1)?;
    assert_row_highlighted(&buf, area, 2)?;
    assert_row_highlighted(&buf, area, 3)?;
    Ok(())
}

#[test]
fn apply_selection_highlight_skips_rows_above_viewport() -> Result<()> {
    let area = Rect::new(0, 0, 5, 5);
    let mut buf = make_test_buffer(5, 5)?;

    // Selection starts at row 0 but viewport starts at offset 3 — nothing visible
    let theme = TuiTheme::default();
    let window = paint_window(&theme, 3, 5, 0);
    RuntimeCoordinator::apply_selection_highlight(
        &mut buf,
        area,
        0, // sel_start
        1, // sel_end
        &window,
        TEST_SELECTION_STYLE,
    );

    // No rows should be highlighted
    for y in 0..5 {
        assert_row_not_highlighted(&buf, area, y)?;
    }
    Ok(())
}

#[test]
fn apply_selection_highlight_skips_rows_below_viewport() -> Result<()> {
    let area = Rect::new(0, 0, 5, 5);
    let mut buf = make_test_buffer(5, 5)?;

    // Selection is at rows 5-6, viewport shows rows 0-4 — nothing visible
    let theme = TuiTheme::default();
    let window = paint_window(&theme, 0, 5, 0);
    RuntimeCoordinator::apply_selection_highlight(
        &mut buf,
        area,
        5, // sel_start
        6, // sel_end
        &window,
        TEST_SELECTION_STYLE,
    );

    for y in 0..5 {
        assert_row_not_highlighted(&buf, area, y)?;
    }
    Ok(())
}

#[test]
fn apply_selection_highlight_single_row() -> Result<()> {
    let area = Rect::new(0, 0, 5, 5);
    let mut buf = make_test_buffer(5, 5)?;

    let theme = TuiTheme::default();
    let window = paint_window(&theme, 0, 5, 0);
    RuntimeCoordinator::apply_selection_highlight(
        &mut buf,
        area,
        2, // sel_start
        2, // sel_end
        &window,
        TEST_SELECTION_STYLE,
    );

    // Only row 2 should be highlighted
    assert_row_not_highlighted(&buf, area, 0)?;
    assert_row_not_highlighted(&buf, area, 1)?;
    assert_row_highlighted(&buf, area, 2)?;
    assert_row_not_highlighted(&buf, area, 3)?;
    assert_row_not_highlighted(&buf, area, 4)?;
    Ok(())
}

#[test]
fn apply_selection_highlight_preserves_non_highlighted_cells() -> Result<()> {
    let area = Rect::new(0, 0, 5, 5);
    let mut buf = make_test_buffer(5, 5)?;

    // Snapshot original styles for rows 0 and 4
    let mut orig_row0 = Vec::new();
    for x in 0..area.width {
        let c = buf.cell((x, 0)).ok_or("cell should be in range")?;
        orig_row0.push((c.fg, c.bg, c.modifier));
    }
    let mut orig_row4 = Vec::new();
    for x in 0..area.width {
        let c = buf.cell((x, 4)).ok_or("cell should be in range")?;
        orig_row4.push((c.fg, c.bg, c.modifier));
    }

    let theme = TuiTheme::default();
    let window = paint_window(&theme, 0, 5, 0);
    RuntimeCoordinator::apply_selection_highlight(
        &mut buf,
        area,
        1, // sel_start
        3, // sel_end
        &window,
        TEST_SELECTION_STYLE,
    );

    // Row 0 should be completely unchanged
    for (x, &(fg, bg, modifier)) in orig_row0.iter().enumerate() {
        let cell = buf.cell((x as u16, 0)).ok_or("cell should be in range")?;
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
        let cell = buf.cell((x as u16, 4)).ok_or("cell should be in range")?;
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
    Ok(())
}

/// A window with top padding paints the content lower than its absolute row
/// number, so the highlight must ride the padding: the painted rows carry the
/// selection style, the padding rows above them do not.
#[test]
fn apply_selection_highlight_rides_the_paint_windows_top_padding() -> Result<()> {
    // -- Setup & Fixtures: content rows 0-1 painted at buffer rows 2-3.
    let area = Rect::new(0, 0, 5, 5);
    let mut buf = make_test_buffer(5, 5)?;
    let theme = TuiTheme::default();
    let window = paint_window(&theme, 0, 2, 2);

    // -- Exec
    RuntimeCoordinator::apply_selection_highlight(
        &mut buf,
        area,
        0, // sel_start
        1, // sel_end
        &window,
        TEST_SELECTION_STYLE,
    );

    // -- Check: the content rows 2 and 3 carry the style, the padding rows do
    // not.
    for y in 0..5u16 {
        if (2..4).contains(&y) {
            assert_row_highlighted(&buf, area, y)?;
        } else {
            assert_row_not_highlighted(&buf, area, y)?;
        }
    }
    Ok(())
}
