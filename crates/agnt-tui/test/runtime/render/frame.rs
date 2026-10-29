use ratatui::layout::{Constraint, Direction, Layout, Rect};
use unicode_width::UnicodeWidthStr;

use crate::rendering::layout::INPUT_MIN_HEIGHT;
use crate::runtime::panels::{
    COMMAND_PALETTE_COLUMN_SPACING, COMMAND_PALETTE_COLUMNS, COMMAND_PALETTE_HIGHLIGHT_SYMBOL,
    MCP_TABLE_COLUMN_SPACING, MCP_TABLE_COLUMNS, MCP_TABLE_HIGHLIGHT_SYMBOL,
};
use crate::runtime::render::frame::{
    ModalPanelKind, modal_rect_for_panel, picker_column_budgets, popup_body_rows,
};
use crate::runtime::render::modals::{MODEL_PICKER_COLUMN_SPACING, MODEL_PICKER_COLUMNS};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

pub const STATUS_TARGET_HEIGHT: u16 = 3;

pub fn main_pane_rects_for_height(main_height: u16) -> (Rect, Rect, Rect, Rect) {
    let main = Rect {
        x: 0,
        y: 0,
        width: 120,
        height: main_height,
    };
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(0),
            Constraint::Min(1),
            Constraint::Length(INPUT_MIN_HEIGHT),
            Constraint::Length(STATUS_TARGET_HEIGHT),
        ])
        .split(main);
    (vertical[0], vertical[1], vertical[2], vertical[3])
}

#[test]
fn test_frame_modal_rect_models_width_clamp() -> Result<()> {
    // -- Setup & Fixtures
    let wide = Rect::new(0, 0, 120, 30);
    let narrow = Rect::new(0, 0, 60, 30);

    // -- Exec
    let wide_popup = modal_rect_for_panel(wide, ModalPanelKind::Models);
    let narrow_popup = modal_rect_for_panel(narrow, ModalPanelKind::Models);

    // -- Check
    assert_eq!(
        wide_popup.width, 80,
        "Models popup must clamp to 80 columns at a 120-column terminal"
    );
    assert_eq!(
        narrow_popup.width, 60,
        "Models popup must be 60 columns at a 60-column terminal"
    );
    Ok(())
}

#[test]
fn test_frame_modal_rect_mcps_width_clamp() -> Result<()> {
    // -- Setup & Fixtures
    let wide = Rect::new(0, 0, 120, 30);
    let narrow = Rect::new(0, 0, 60, 30);

    // -- Exec
    let wide_popup = modal_rect_for_panel(wide, ModalPanelKind::Mcps);
    let narrow_popup = modal_rect_for_panel(narrow, ModalPanelKind::Mcps);

    // -- Check
    assert_eq!(
        wide_popup.width, 72,
        "MCPs popup must clamp to 72 columns at a 120-column terminal"
    );
    assert_eq!(
        narrow_popup.width, 60,
        "MCPs popup must be 60 columns at a 60-column terminal"
    );
    assert_eq!(
        wide_popup.height, narrow_popup.height,
        "MCPs popup height must not depend on the terminal width"
    );
    Ok(())
}

#[test]
fn test_frame_modal_rect_other_panels_keep_shared_clamp() -> Result<()> {
    // -- Setup & Fixtures
    let area = Rect::new(0, 0, 120, 30);
    let panels = [
        ModalPanelKind::Skills,
        ModalPanelKind::Agents,
        ModalPanelKind::Sessions,
    ];

    // -- Exec & Check
    for panel in panels {
        let popup = modal_rect_for_panel(area, panel);
        assert_eq!(
            popup.width, 90,
            "{panel:?} popup must keep the shared 90-column maximum"
        );
    }
    Ok(())
}

/// The themes popup fits its single-column 4-item list with no footer: at a
/// wide terminal the popup clamps to its own 32-column maximum and 12-row
/// maximum, and below the designed minimum it takes the terminal width.
#[test]
fn test_frame_modal_rect_themes_clamp() -> Result<()> {
    // -- Setup & Fixtures
    let wide = Rect::new(0, 0, 120, 30);
    let narrow = Rect::new(0, 0, 24, 30);
    let short = Rect::new(0, 0, 120, 8);

    // -- Exec
    let wide_popup = modal_rect_for_panel(wide, ModalPanelKind::Themes);
    let narrow_popup = modal_rect_for_panel(narrow, ModalPanelKind::Themes);
    let short_popup = modal_rect_for_panel(short, ModalPanelKind::Themes);

    // -- Check
    assert_eq!(
        (wide_popup.width, wide_popup.height),
        (32, 12),
        "Themes popup must clamp to 32x12 at a wide terminal"
    );
    assert_eq!(
        narrow_popup.width, 24,
        "Themes popup must take the terminal width below the 28-column minimum"
    );
    assert_eq!(
        short_popup.height, 8,
        "Themes popup must keep its 8-row content minimum at a short terminal"
    );
    Ok(())
}

/// The models table must keep CTX at 6 cells and PRICE at 16 cells for every
/// table width the models popup can produce (popup 52..=80 → table 50..=78),
/// and the MODEL column must never drop below its 20-cell minimum (task
/// 5f57acbb).
#[test]
fn test_frame_models_picker_column_budgets_keep_ctx_and_price() -> Result<()> {
    // -- Setup & Fixtures: the constraints the picker actually renders.
    let columns = MODEL_PICKER_COLUMNS;
    let selection_width = 2u16;
    let spacing = MODEL_PICKER_COLUMN_SPACING;

    // -- Exec & Check
    for table_width in 50u16..=78 {
        let budgets = picker_column_budgets(table_width, selection_width, spacing, &columns);
        let model = *budgets.first().ok_or("should resolve the MODEL column")?;
        let ctx = *budgets.get(1).ok_or("should resolve the CTX column")?;
        let price = *budgets.get(2).ok_or("should resolve the PRICE column")?;
        assert_eq!(
            ctx, 6,
            "CTX must keep its 6-cell budget at table width {table_width}; budgets {budgets:?}"
        );
        assert_eq!(
            price, 16,
            "PRICE must keep its 16-cell budget at table width {table_width}; budgets {budgets:?}"
        );
        assert!(
            model >= 20,
            "MODEL must keep its 20-cell minimum at table width {table_width}; budgets {budgets:?}"
        );
        let occupied = budgets.iter().sum::<usize>()
            + usize::from(selection_width)
            + usize::from(spacing) * budgets.len().saturating_sub(1);
        assert!(
            occupied <= usize::from(table_width),
            "budgets {budgets:?} must fit a {table_width}-wide table; occupied {occupied}"
        );
    }
    Ok(())
}

/// A popup without a footer passes zero: the ladder has a one-row hint band and
/// the body takes every remaining row.
#[test]
fn test_frame_popup_body_rows_without_bottom_band() -> Result<()> {
    // -- Setup & Fixtures
    let inner = Rect::new(2, 3, 40, 10);

    // -- Exec
    let rows = popup_body_rows(inner, 0);

    // -- Check
    assert_eq!(
        rows.len(),
        2,
        "a zero-row bottom band must not claim a rect; got {rows:?}"
    );
    assert_eq!(
        rows[0],
        Rect::new(2, 3, 40, 1),
        "the hint band must be the inner area's first row"
    );
    assert_eq!(
        rows[1],
        Rect::new(2, 4, 40, 9),
        "the body must take every row below the hint band"
    );
    assert_eq!(
        rows[1].bottom(),
        inner.bottom(),
        "the body must reach the inner area's last row"
    );
    Ok(())
}

/// A popup with a footer passes its row count: the ladder adds a bottom band of
/// exactly that many rows, and its last row is the inner area's last row.
#[test]
fn test_frame_popup_body_rows_with_bottom_band() -> Result<()> {
    // -- Setup & Fixtures
    let inner = Rect::new(2, 3, 40, 10);

    // -- Exec
    let rows = popup_body_rows(inner, 3);

    // -- Check
    assert_eq!(
        rows.len(),
        3,
        "a three-row bottom band must claim a rect; got {rows:?}"
    );
    assert_eq!(
        rows[0],
        Rect::new(2, 3, 40, 1),
        "the hint band must be the inner area's first row"
    );
    assert_eq!(
        rows[1],
        Rect::new(2, 4, 40, 6),
        "the body must take every row between the two bands"
    );
    assert_eq!(
        rows[2],
        Rect::new(2, 10, 40, 3),
        "the bottom band must be exactly bottom_rows tall"
    );
    assert_eq!(
        rows[2].bottom(),
        inner.bottom(),
        "the bottom band must end on the inner area's last row"
    );
    Ok(())
}

/// The derived budgets must fit every table width the agents picker can
/// produce and must keep the two Min columns at their lower bounds.
#[test]
fn test_frame_agents_picker_column_budgets_sweep() -> Result<()> {
    // -- Setup & Fixtures
    let columns = [
        Constraint::Min(12),
        Constraint::Min(20),
        Constraint::Length(1),
    ];
    let selection_width = 2usize;
    let spacing = 2usize;
    let glyph_width = 1usize;

    // -- Exec & Check
    for table_width in 37u16..=88 {
        let budgets = picker_column_budgets(table_width, 2, 1, &columns);
        let name = *budgets.first().ok_or("should resolve the name column")?;
        let description = *budgets
            .get(1)
            .ok_or("should resolve the description column")?;
        let occupied = name + description + selection_width + spacing + glyph_width;
        assert!(
            occupied <= usize::from(table_width),
            "budgets {budgets:?} must fit a {table_width}-wide table; occupied {occupied}"
        );
        assert!(
            name >= 12,
            "name budget {name} must honour the 12-column minimum at table width {table_width}"
        );
        assert!(
            description >= 20,
            "description budget {description} must honour the 20-column minimum at table width {table_width}"
        );
    }

    let narrow = picker_column_budgets(43, 2, 1, &columns);
    assert_eq!(
        narrow,
        vec![18, 20, 1],
        "a 43-wide table must pin Description at 20 and leave Agent 18"
    );
    Ok(())
}

/// The palette's derived budgets must follow the columns the solver resolves:
/// the Summary minimum holds below a 24-column table while the Action column
/// takes the remainder, and from 24 columns up the Summary column absorbs the
/// excess (task 74caeec7).
#[test]
fn test_frame_command_palette_column_budgets_sweep() -> Result<()> {
    // -- Setup & Fixtures: the constraints the palette actually renders.
    let columns = COMMAND_PALETTE_COLUMNS;
    let selection_width =
        usize::from(UnicodeWidthStr::width(COMMAND_PALETTE_HIGHLIGHT_SYMBOL) as u16);
    let spacing = usize::from(COMMAND_PALETTE_COLUMN_SPACING);

    // -- Exec & Check: the palette popup spans 20..=48 → table 18..=46.
    for table_width in 18u16..=46 {
        let budgets = picker_column_budgets(
            table_width,
            UnicodeWidthStr::width(COMMAND_PALETTE_HIGHLIGHT_SYMBOL) as u16,
            COMMAND_PALETTE_COLUMN_SPACING,
            &columns,
        );
        let action = *budgets.first().ok_or("should resolve the Action column")?;
        let summary = *budgets.get(1).ok_or("should resolve the Summary column")?;
        assert_eq!(
            summary,
            usize::from(table_width).saturating_sub(12).max(12),
            "Summary must hold its 12-cell minimum below a 24-column table and absorb the excess above it; table width {table_width}, budgets {budgets:?}"
        );
        assert_eq!(
            action,
            usize::from(table_width).saturating_sub(16).min(8),
            "Action must hold its 8-cell length once the table fits and take the remainder below it; table width {table_width}, budgets {budgets:?}"
        );
        assert!(
            action + summary + selection_width + spacing <= usize::from(table_width),
            "budgets {budgets:?} must fit a {table_width}-wide table"
        );
    }
    Ok(())
}

/// The MCP table's budgets must follow the columns the solver resolves: from a
/// 42-column table up the Name column absorbs the excess and the two Length
/// columns keep 14 and 6, so the Name budget stays the 24-cell-overhead value
/// the designed popup range (56..=72 → table 54..=70) renders today. Below 42
/// columns the Name column pins at its 18-cell minimum and the Length columns
/// collapse, so the budget follows the pinned column instead of undershooting
/// (task 41ba78ae).
#[test]
fn test_frame_mcp_table_column_budgets_sweep() -> Result<()> {
    // -- Setup & Fixtures: the constraints the MCP table actually renders.
    let columns = MCP_TABLE_COLUMNS;
    let selection_width = usize::from(UnicodeWidthStr::width(MCP_TABLE_HIGHLIGHT_SYMBOL) as u16);

    // -- Exec & Check: every table width, from a collapsed table up to the
    // 70-column cap.
    for table_width in 0u16..=70 {
        let budgets = picker_column_budgets(
            table_width,
            UnicodeWidthStr::width(MCP_TABLE_HIGHLIGHT_SYMBOL) as u16,
            MCP_TABLE_COLUMN_SPACING,
            &columns,
        );
        let name = *budgets.first().ok_or("should resolve the Name column")?;
        // Above the feasible width the Name column takes every leftover cell;
        // below it the column pins at 18 once the row can hold it and shrinks
        // with the row until then.
        let expected_name = if table_width >= 42 {
            usize::from(table_width).saturating_sub(24)
        } else {
            usize::from(table_width.saturating_sub(4)).min(18)
        };
        assert_eq!(
            name, expected_name,
            "Name must follow the resolved column at every table width; table width {table_width}, budgets {budgets:?}"
        );
        assert!(
            budgets.iter().sum::<usize>()
                <= usize::from(table_width.saturating_sub(selection_width as u16)),
            "budgets {budgets:?} must fit the columns area of a {table_width}-wide table"
        );
    }

    // -- Check: the designed range keeps today's cells, the degenerate band
    // pins Name, and the Length columns collapse in column order.
    let budgets_at = |table_width: u16| {
        picker_column_budgets(
            table_width,
            UnicodeWidthStr::width(MCP_TABLE_HIGHLIGHT_SYMBOL) as u16,
            MCP_TABLE_COLUMN_SPACING,
            &columns,
        )
    };
    assert_eq!(
        budgets_at(54),
        vec![30, 14, 6],
        "a 54-wide table must keep Visible tools at 14 and Status at 6 and leave Name 30"
    );
    assert_eq!(
        budgets_at(70),
        vec![46, 14, 6],
        "a 70-wide table must leave Name the 46 cells the capped popup renders today"
    );
    assert_eq!(
        budgets_at(42),
        vec![18, 14, 6],
        "the last feasible table must leave Name exactly its 18-cell minimum"
    );
    assert_eq!(
        budgets_at(32),
        vec![18, 5, 5],
        "a 32-wide table must pin Name at 18 and collapse the Length columns"
    );
    Ok(())
}
