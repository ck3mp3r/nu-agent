use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(test)]
use ratatui::layout::{Constraint, Direction, Layout, Rect};

#[cfg(not(test))]
use ratatui::layout::Rect;

use crate::agent::ui::tui::rendering::layout::{INPUT_MIN_HEIGHT, TRANSCRIPT_MIN_HEIGHT};

pub(super) const IN_PROGRESS_SPINNER_FRAMES: [&str; 10] =
    ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

const STATUS_TARGET_HEIGHT: u16 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ModalPanelKind {
    CommandPalette,
    Help,
    Status,
    Mcps,
    Skills,
    Models,
}

pub(super) fn modal_rect_for_panel(area: Rect, panel: ModalPanelKind) -> Rect {
    let (min_w, max_w, min_h, max_h) = match panel {
        ModalPanelKind::CommandPalette => (20u16, 48u16, 5u16, 10u16),
        ModalPanelKind::Help => (72u16, 112u16, 18u16, 34u16),
        ModalPanelKind::Status => (42u16, 72u16, 8u16, 14u16),
        ModalPanelKind::Mcps | ModalPanelKind::Skills | ModalPanelKind::Models => {
            (56u16, 90u16, 10u16, 20u16)
        }
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

pub(super) fn vertical_heights_for_main_with_input(
    main_height: u16,
    input_target_height: u16,
) -> (u16, u16, u16, u16) {
    if main_height == 0 {
        return (0, 0, 0, 0);
    }

    let header = 0;
    let mut remaining = main_height;

    let input = input_target_height.max(INPUT_MIN_HEIGHT).min(remaining);
    remaining = remaining.saturating_sub(input);

    let min_transcript = u16::from(remaining > 0).min(TRANSCRIPT_MIN_HEIGHT);
    let status = STATUS_TARGET_HEIGHT.min(remaining.saturating_sub(min_transcript));
    let transcript = remaining.saturating_sub(status);

    (header, transcript, status, input)
}

#[cfg(test)]
pub(super) fn main_pane_rects_for_height(main_height: u16) -> (Rect, Rect, Rect, Rect) {
    let main = Rect {
        x: 0,
        y: 0,
        width: 120,
        height: main_height,
    };
    let (header, transcript, status, input) =
        vertical_heights_for_main_with_input(main.height, INPUT_MIN_HEIGHT);
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header),
            Constraint::Length(transcript),
            Constraint::Length(input),
            Constraint::Length(status),
        ])
        .split(main);
    (vertical[0], vertical[1], vertical[2], vertical[3])
}
