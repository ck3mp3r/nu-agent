//! Input caret cell mapping: which buffer cell the caret paints.

use super::bottom_box_support::*;

use crate::runtime::render::bottom_box::caret_cell_position;

use ratatui::layout::{Position, Rect};

/// WHEN the caret's row lies inside the textarea rect, THE caret mapping SHALL
/// return the cell at that row and column; WHEN the column runs past the right
/// edge, THE mapping SHALL clamp it to the last column; WHEN the row runs past
/// the bottom edge, THE mapping SHALL return `None` so the caret is skipped.
#[test]
fn test_bottom_box_caret_cell_position_maps_clamps_and_skips() -> Result<()> {
    // -- Setup & Fixtures: a textarea three rows tall, ten columns wide.
    let rect = Rect::new(4, 5, 10, 3);

    // -- Exec & Check: the first cell of the first row.
    assert_eq!(
        caret_cell_position(rect, 0, 0),
        Some(Position { x: 4, y: 5 }),
        "the caret at row 0, column 0 must map to the rect's first cell"
    );

    // -- Exec & Check: a cell on the last row.
    assert_eq!(
        caret_cell_position(rect, 2, 3),
        Some(Position { x: 7, y: 7 }),
        "the caret must map its row and column onto the rect"
    );

    // -- Exec & Check: a column past the right edge clamps to the last column.
    assert_eq!(
        caret_cell_position(rect, 0, 40),
        Some(Position { x: 13, y: 5 }),
        "a column beyond the rect must clamp to the last column, not leave the rect"
    );

    // -- Exec & Check: a row past the bottom edge is skipped, never clamped.
    assert_eq!(
        caret_cell_position(rect, 3, 0),
        None,
        "a row below the rect must skip the caret cell"
    );

    // -- Exec & Check: a rect with no rows paints no caret cell.
    assert_eq!(
        caret_cell_position(Rect::new(4, 5, 0, 0), 0, 0),
        None,
        "an empty rect must skip the caret cell"
    );
    Ok(())
}
