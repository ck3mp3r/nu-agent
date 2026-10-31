use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Text},
    widgets::Paragraph,
};

use crate::rendering::{
    chrome::{Chrome, ContainerBorder},
    theme::TuiTheme,
};

pub(super) fn render_scroll_text_panel(
    frame: &mut Frame,
    area: Rect,
    title: impl Into<Line<'static>>,
    lines: Text<'_>,
    scroll: usize,
    theme: &TuiTheme,
) {
    let inner = render_modal_frame(frame, area, title, theme);
    frame.render_widget(
        Paragraph::new(lines).scroll((scroll.min(u16::MAX as usize) as u16, 0)),
        inner,
    );
}

pub(super) fn render_modal_frame(
    frame: &mut Frame,
    area: Rect,
    title: impl Into<Line<'static>>,
    theme: &TuiTheme,
) -> Rect {
    Chrome::default()
        .with_clear(true)
        .with_bg(Style::default().bg(theme.base))
        .with_border(ContainerBorder::Rounded)
        .with_border_style(theme.subtle_meta)
        .with_title(title)
        .with_title_style(theme.subtle_meta)
        .draw(frame, area)
}

/// Render a popup's one-row hint band: the line at the rect's first row in the
/// popup's meta style, with no background.
///
/// Every popup opens with one such row — the picker's query line, the MCPs
/// controls line, the themes prompt — so the band is one component.
pub(super) fn render_popup_hint_row(
    frame: &mut Frame,
    rect: Rect,
    line: Line<'static>,
    theme: &TuiTheme,
) {
    frame.render_widget(Paragraph::new(line).style(theme.subtle_meta), rect);
}

/// Render a popup's empty-state message: one plain line at the rect's first row
/// in the popup's meta foreground, with no background and no bold.
///
/// Every picker paints its "nothing to pick" message this way, so the empty and
/// filled states share one text surface.
pub(super) fn render_empty_state(frame: &mut Frame, rect: Rect, message: &str, theme: &TuiTheme) {
    frame.render_widget(
        Paragraph::new(Line::from(message.to_string())).style(theme.tool_meta),
        rect,
    );
}

/// Paint the user rail's gutter cells on one buffer row: the `╎` marker slot
/// plus its trailing blank, in the user lane's prefix style — the same span the
/// message widget renders on every row of a user turn. The user background box
/// is taller than the turn: the gap rows that bleed `row_user_bg` and the
/// queued stack's padding rows sit inside the box, so they paint this slot to
/// keep the rail continuous down the whole box (task 4d99956c).
pub(crate) fn paint_user_rail_row(
    buf: &mut ratatui::buffer::Buffer,
    y: u16,
    width: usize,
    theme: &TuiTheme,
) {
    if y >= buf.area.bottom() {
        return;
    }
    let width = u16::try_from(width).unwrap_or(u16::MAX).min(buf.area.width);
    buf.set_stringn(
        buf.area.left(),
        y,
        "╎ ",
        usize::from(width),
        theme.lane_prefix_user,
    );
}

pub(crate) mod bottom_box;
pub(super) mod frame;
mod modals;
mod picker_footer;
mod picker_table;

pub(crate) use frame::current_time_millis;
pub(crate) use picker_footer::PickerFooter;
pub(super) use picker_table::section_head_count;
pub(crate) mod transcript;

#[cfg(test)]
#[path = "../../../test/runtime/render/bottom_box/support.rs"]
mod bottom_box_support;

#[cfg(test)]
#[path = "../../../test/runtime/render/bottom_box/stack.rs"]
mod bottom_box_stack_test;

#[cfg(test)]
#[path = "../../../test/runtime/render/bottom_box/status.rs"]
mod bottom_box_status_test;

#[cfg(test)]
#[path = "../../../test/runtime/render/bottom_box/input_box.rs"]
mod bottom_box_input_box_test;

#[cfg(test)]
#[path = "../../../test/runtime/render/bottom_box/caret.rs"]
mod bottom_box_caret_test;

#[cfg(test)]
#[path = "../../../test/runtime/render/empty_state.rs"]
mod empty_state_test;

#[cfg(test)]
#[path = "../../../test/runtime/render/picker_footer.rs"]
mod picker_footer_test;

#[cfg(test)]
#[path = "../../../test/runtime/render/picker_table.rs"]
mod picker_table_test;

#[cfg(test)]
#[path = "../../../test/runtime/render/popup_hint_row.rs"]
mod popup_hint_row_test;

#[cfg(test)]
#[path = "../../../test/runtime/render/frame.rs"]
pub(super) mod frame_test;

#[cfg(test)]
#[path = "../../../test/runtime/render/selection_render.rs"]
pub(super) mod selection_render_test;

#[cfg(test)]
#[path = "../../../test/runtime/render/transcript.rs"]
mod transcript_test;
