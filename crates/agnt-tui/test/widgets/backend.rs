//! Backend seam tests: absolute rows, clearing, and plan parity.

use super::support::*;
use crate::widgets::RatatuiBackend;
use agnt_core::transcript::ir::MessageRole;
use agnt_render::{Gallery, LayoutPlanner, RenderBackend, RowRect};

/// WHEN the backend renders an entry at a non-zero row, THE entry SHALL land on
/// that absolute frame row.
#[test]
fn test_widgets_backend_renders_at_the_absolute_rect_row() -> Result<()> {
    // -- Setup & Fixtures
    let entry = message_entry(MessageRole::User, "Hello there");

    // -- Exec
    let (backend, _rows) = render_at(&entry, 3)?;

    // -- Check
    for y in 0..3u16 {
        assert!(
            row_text(&backend, y).trim().is_empty(),
            "row {y} stays blank above the entry"
        );
    }
    assert!(row_text(&backend, 3).contains("Hello there"));
    Ok(())
}

/// WHEN the backend is cleared, THE painted surface SHALL be blank again.
#[test]
fn test_widgets_backend_clear_blanks_the_surface() -> Result<()> {
    // -- Setup & Fixtures
    let entry = message_entry(MessageRole::User, "Hello there");

    // -- Exec
    let (mut backend, _rows) = render_at(&entry, 0)?;
    backend.clear();

    // -- Check
    for y in 0..HEIGHT {
        assert!(
            row_text(&backend, y).trim().is_empty(),
            "row {y} is blank after clear"
        );
    }
    Ok(())
}

/// WHEN the planner plans the gallery samples and the backend paints them, THE
/// painted rows SHALL land exactly where the plan puts them.
#[test]
fn test_widgets_backend_paints_the_planned_rects() -> Result<()> {
    // -- Setup & Fixtures
    let entries: Vec<_> = Gallery::samples()
        .into_iter()
        .map(|sample| sample.entry)
        .collect();
    let width = usize::from(WIDTH);
    let plan = LayoutPlanner::plan(&entries, width, &blank_backend());
    // The frame spans the plan exactly: the gallery's row count is not a
    // fixture constant, so a new sample must not overflow the paint area.
    let planned_rows = plan
        .entries
        .iter()
        .map(|entry_plan| entry_plan.rect.row + entry_plan.rect.rows)
        .max()
        .unwrap_or(1);
    let mut planned = RatatuiBackend::new(WIDTH, u16::try_from(planned_rows.max(1))?);

    // -- Exec: paint every entry at its planned rect.
    for entry_plan in &plan.entries {
        let entry = entries
            .get(entry_plan.index)
            .ok_or("planned index must exist")?;
        planned.render(entry, entry_plan.rect, &test_theme());
    }

    // -- Check: each entry's cells match a fresh render at row 0.
    for entry_plan in &plan.entries {
        let entry = entries
            .get(entry_plan.index)
            .ok_or("planned index must exist")?;
        // The fresh frame is sized to the entry's rect, not the fixture height:
        // the gallery's row counts are not fixture constants, so a new sample
        // must not overrun the surface.
        let alone_rows = u16::try_from(entry_plan.rect.rows)?;
        let mut alone = RatatuiBackend::new(WIDTH, alone_rows.max(1));
        alone.render(
            entry,
            RowRect::new(0, width, entry_plan.rect.rows),
            &test_theme(),
        );
        for offset in 0..entry_plan.rect.rows {
            let planned_y = u16::try_from(entry_plan.rect.row + offset)?;
            let alone_y = u16::try_from(offset)?;
            for x in 0..WIDTH {
                assert_eq!(
                    planned.buffer()[(x, planned_y)].symbol(),
                    alone.buffer()[(x, alone_y)].symbol(),
                    "symbol at ({x}, {planned_y})"
                );
                assert_eq!(
                    planned.buffer()[(x, planned_y)].style(),
                    alone.buffer()[(x, alone_y)].style(),
                    "style at ({x}, {planned_y})"
                );
            }
        }
    }
    Ok(())
}
