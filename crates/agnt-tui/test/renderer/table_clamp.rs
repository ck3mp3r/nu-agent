//! Table clamping: the projector's clamp target is the renderer's wrap budget,
//! so a table that "fits" the canvas is never re-wrapped and its box drawing
//! stays intact.

use super::support::*;
use agnt_core::transcript::ir::MessageRole;

use crate::widgets::RatatuiBackend;

/// Natural table width 37 (4 columns, 6 display columns each): wider than the
/// wrap budget of a 40-column canvas but inside the canvas itself.
const WIDE_TABLE: &str = "| Name | Status | Owner | Notes |\n\
| --- | --- | --- | --- |\n\
| abcdef | ghijkl | mnopqr | stuvwx |";

const CANVAS: u16 = 40;

/// WHEN a table's natural width sits between the wrap budget and the canvas
/// width, THE renderer SHALL clamp it to the wrap budget: no row is re-wrapped
/// and the top and bottom borders stay on one row.
#[test]
fn test_renderer_table_clamp_keeps_borders_unbroken_at_wrap_budget() -> Result<()> {
    // -- Setup & Fixtures
    let entry = message_entry(MessageRole::Assistant, WIDE_TABLE);

    // -- Exec
    let (backend, rows) = render_in(&entry, CANVAS)?;

    // -- Check: no row exceeds the canvas.
    let texts: Vec<String> = (0..u16::try_from(rows)?)
        .map(|y| row_text_in(&backend, CANVAS, y).trim_end().to_string())
        .collect();
    for (y, row) in texts.iter().enumerate() {
        assert!(
            row.chars().count() <= usize::from(CANVAS),
            "row {y} exceeds the canvas: {row:?}"
        );
    }

    // -- Check: the top and bottom borders are each one unbroken row.
    let top = texts
        .iter()
        .find(|row| row.contains('\u{256d}'))
        .ok_or("the table must paint a top border")?;
    assert!(
        top.contains('\u{256e}'),
        "the top border must stay on one row: {top:?}"
    );
    let bottom = texts
        .iter()
        .find(|row| row.contains('\u{2570}'))
        .ok_or("the table must paint a bottom border")?;
    assert!(
        bottom.contains('\u{256f}'),
        "the bottom border must stay on one row: {bottom:?}"
    );
    Ok(())
}

/// WHEN a table is wider than the wrap budget, THE renderer SHALL keep every
/// column — cell contents wrap inside their column and no border fragment splits.
#[test]
fn test_renderer_table_clamp_keeps_all_columns_within_canvas() -> Result<()> {
    // -- Setup & Fixtures
    let entry = message_entry(MessageRole::Assistant, WIDE_TABLE);

    // -- Exec
    let (backend, rows) = render_in(&entry, CANVAS)?;
    let texts: Vec<String> = (0..u16::try_from(rows)?)
        .map(|y| row_text_in(&backend, CANVAS, y).trim_end().to_string())
        .collect();
    let joined = texts.join("\n");

    // -- Check: every column survives.
    for label in ["Name", "Status", "Owner", "Notes"] {
        assert!(
            joined.contains(label),
            "column header {label} must render; got:\n{joined}"
        );
    }
    assert!(
        !joined.contains("hidden"),
        "no column is dropped, so no hidden-columns marker; got:\n{joined}"
    );

    // -- Check: each box row keeps the full border set — 4 columns need 5 `│`.
    let box_rows: Vec<&String> = texts
        .iter()
        .filter(|row| row.contains('\u{2502}'))
        .collect();
    assert!(
        box_rows.len() >= 3,
        "header plus the wrapped data rows expected; got:\n{joined}"
    );
    for row in box_rows {
        assert_eq!(
            row.matches('\u{2502}').count(),
            5,
            "every physical row keeps all 5 cell separators: {row:?}"
        );
    }
    Ok(())
}

/// WHEN the backend measures a clamped table and paints it, THE painted row count
/// SHALL equal the measured one — both project at the wrap budget.
#[test]
fn test_renderer_table_clamp_measure_matches_painted_rows() -> Result<()> {
    // -- Setup & Fixtures
    let entry = message_entry(MessageRole::Assistant, WIDE_TABLE);

    // -- Exec
    let (backend, rows) = render_in(&entry, CANVAS)?;

    // -- Check
    assert_eq!(painted_rows_in(&backend, CANVAS), rows);
    Ok(())
}

/// The rendered rows a backend `width` columns wide painted.
fn painted_rows_in(backend: &RatatuiBackend, width: u16) -> usize {
    let mut count = 0;
    for y in 0..HEIGHT {
        let painted = (0..width).any(|x| {
            let cell = &backend.buffer()[(x, y)];
            cell.symbol() != " " || !is_untinted(cell.style().bg)
        });
        if !painted {
            break;
        }
        count += 1;
    }
    count
}
