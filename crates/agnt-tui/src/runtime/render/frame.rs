use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::layout::{Constraint, Direction, Flex, Layout, Rect};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModalPanelKind {
    CommandPalette,
    Help,
    Status,
    Mcps,
    Skills,
    Models,
    Agents,
    Sessions,
    Themes,
}

pub(crate) fn modal_rect_for_panel(area: Rect, panel: ModalPanelKind) -> Rect {
    let (min_w, max_w, min_h, max_h) = match panel {
        ModalPanelKind::CommandPalette => (20u16, 48u16, 5u16, 10u16),
        ModalPanelKind::Help => (72u16, 112u16, 18u16, 34u16),
        ModalPanelKind::Status => (42u16, 72u16, 8u16, 14u16),
        ModalPanelKind::Models => (60u16, 80u16, 10u16, 20u16),
        ModalPanelKind::Mcps => (56u16, 72u16, 10u16, 20u16),
        ModalPanelKind::Skills | ModalPanelKind::Agents | ModalPanelKind::Sessions => {
            (56u16, 90u16, 10u16, 20u16)
        }
        ModalPanelKind::Themes => (28u16, 32u16, 8u16, 12u16),
    };

    let popup_width = area
        .width
        .clamp(min_w.min(area.width), max_w.min(area.width));
    let popup_height = area
        .height
        .clamp(min_h.min(area.height), max_h.min(area.height));
    Rect {
        x: area
            .x
            .saturating_add(area.width.saturating_sub(popup_width) / 2),
        y: area
            .y
            .saturating_add(area.height.saturating_sub(popup_height) / 2),
        width: popup_width,
        height: popup_height,
    }
}

/// Resolved widths of a table's data columns, in display cells.
///
/// Replicates the two-stage split `ratatui` performs for a picker table: the
/// selection column is carved out first, then the column constraints are laid
/// out over the remainder with the table's flex and column spacing. Callable
/// from the runtime panels as well as this module so a table's cell budgets
/// come from the constraints the solver resolves.
pub(crate) fn picker_column_budgets(
    table_width: u16,
    selection_width: u16,
    spacing: u16,
    columns: &[Constraint],
) -> Vec<usize> {
    let [_selection_area, columns_area] =
        Layout::horizontal([Constraint::Length(selection_width), Constraint::Fill(0)])
            .areas(Rect::new(0, 0, table_width, 1));
    Layout::horizontal(columns)
        .flex(Flex::Start)
        .spacing(spacing)
        .split(columns_area)
        .iter()
        .map(|rect| usize::from(rect.width))
        .collect()
}

/// Split a popup's inner area into its render ladder: the one-row hint band on
/// top, the body underneath, and — only when `bottom_rows` is non-zero — a
/// bottom band of exactly `bottom_rows` rows pinned to the inner area's last
/// row.
///
/// A popup without a footer passes zero and hands every remaining row to the
/// body, so the body rect is the same Rect a two-row ladder produces.
pub(super) fn popup_body_rows(inner: Rect, bottom_rows: u16) -> Rc<[Rect]> {
    let mut constraints = vec![Constraint::Length(1), Constraint::Min(0)];
    if bottom_rows > 0 {
        constraints.push(Constraint::Length(bottom_rows));
    }
    Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner)
}

pub(crate) fn current_time_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
