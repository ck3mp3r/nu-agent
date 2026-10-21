use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::rendering::layout::INPUT_MIN_HEIGHT;
use crate::runtime::render::frame::{ModalPanelKind, modal_rect_for_panel};

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
fn test_frame_modal_rect_other_panels_keep_shared_clamp() -> Result<()> {
    // -- Setup & Fixtures
    let area = Rect::new(0, 0, 120, 30);
    let panels = [
        ModalPanelKind::Mcps,
        ModalPanelKind::Skills,
        ModalPanelKind::Agents,
        ModalPanelKind::Sessions,
        ModalPanelKind::Themes,
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
