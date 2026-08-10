use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::rendering::layout::INPUT_MIN_HEIGHT;

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
