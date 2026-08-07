use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::layout::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ModalPanelKind {
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

pub(super) fn modal_rect_for_panel(area: Rect, panel: ModalPanelKind) -> Rect {
    let (min_w, max_w, min_h, max_h) = match panel {
        ModalPanelKind::CommandPalette => (20u16, 48u16, 5u16, 10u16),
        ModalPanelKind::Help => (72u16, 112u16, 18u16, 34u16),
        ModalPanelKind::Status => (42u16, 72u16, 8u16, 14u16),
        ModalPanelKind::Mcps
        | ModalPanelKind::Skills
        | ModalPanelKind::Models
        | ModalPanelKind::Agents
        | ModalPanelKind::Sessions
        | ModalPanelKind::Themes => (56u16, 90u16, 10u16, 20u16),
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

pub(super) fn current_time_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
