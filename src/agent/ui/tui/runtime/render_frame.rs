use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(test)]
use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::agent::ui::tui::rendering::layout::{INPUT_MIN_HEIGHT, TRANSCRIPT_MIN_HEIGHT};

pub(super) const IN_PROGRESS_SPINNER_FRAMES: [&str; 10] =
    ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

const HEADER_HEIGHT: u16 = 1;
const STATUS_TARGET_HEIGHT: u16 = 1;

pub(super) fn current_time_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

pub(super) fn vertical_heights_for_main(main_height: u16) -> (u16, u16, u16, u16) {
    vertical_heights_for_main_with_input(main_height, INPUT_MIN_HEIGHT)
}

pub(super) fn vertical_heights_for_main_with_input(
    main_height: u16,
    input_target_height: u16,
) -> (u16, u16, u16, u16) {
    if main_height == 0 {
        return (0, 0, 0, 0);
    }

    let header = HEADER_HEIGHT.min(main_height);
    let mut remaining = main_height.saturating_sub(header);

    let input = input_target_height.max(INPUT_MIN_HEIGHT).min(remaining);
    remaining = remaining.saturating_sub(input);

    let min_transcript = u16::from(remaining > 0).min(TRANSCRIPT_MIN_HEIGHT);
    let status = STATUS_TARGET_HEIGHT.min(remaining.saturating_sub(min_transcript));
    let transcript = remaining.saturating_sub(status);

    (header, transcript, status, input)
}

pub(super) fn transcript_height_for_main(main_height: u16) -> u16 {
    let (_, transcript, _, _) = vertical_heights_for_main(main_height);
    transcript
}

#[cfg(test)]
pub(super) fn main_pane_rects_for_height(main_height: u16) -> (Rect, Rect, Rect, Rect) {
    let main = Rect {
        x: 0,
        y: 0,
        width: 120,
        height: main_height,
    };
    let (header, transcript, status, input) = vertical_heights_for_main(main.height);
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
