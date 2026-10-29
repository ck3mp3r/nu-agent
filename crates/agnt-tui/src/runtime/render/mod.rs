use ratatui::{
    Frame,
    layout::{Margin, Rect},
    style::Style,
    text::{Line, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::rendering::theme::TuiTheme;

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
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.base)),
        area,
    );
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_set(ratatui::symbols::border::ROUNDED)
            .border_style(theme.subtle_meta)
            .title(title),
        area,
    );
    area.inner(Margin {
        vertical: 1,
        horizontal: 1,
    })
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

/// The user rail's gutter row: the `╎` marker slot plus its trailing blank, in
/// the user lane's prefix style — the same span `layout()` renders on every row
/// of a user block. The user background box is taller than the block: the
/// separator rows that bleed `row_user_bg` and the queued stack's padding rows
/// sit inside the box, so they paint this line to keep the rail continuous down
/// the whole box (task 4d99956c).
pub(crate) fn user_rail_line(theme: &TuiTheme) -> Line<'static> {
    Line::from(ratatui::text::Span::styled(
        "╎ ".to_string(),
        theme.lane_prefix_user,
    ))
}

/// Expand entry_indices from pre-wrap line count to post-wrap visual row count.
/// Each pre-wrap line may wrap into multiple visual rows; the entry index is
/// replicated for each resulting visual row.
pub(super) fn expand_to_visual_rows(
    entry_indices: Vec<usize>,
    lines: &[Line<'static>],
    width: usize,
) -> Vec<usize> {
    let mut expanded = Vec::with_capacity(lines.len().max(entry_indices.len()));
    for (i, line) in lines.iter().enumerate() {
        let entry_idx = *entry_indices.get(i).unwrap_or(&0);
        let visual_rows = single_line_visual_row_count(line, width);
        for _ in 0..visual_rows {
            expanded.push(entry_idx);
        }
    }
    expanded
}

/// Expand a per-line background override vector to visual rows, mirroring
/// [`expand_to_visual_rows`]: each pre-wrap line replicates its background
/// for every visual row the line wraps into. Used so a pinned separator-row
/// background survives wrapping.
pub(super) fn expand_bgs_to_visual_rows(
    line_bgs: &[Option<ratatui::style::Color>],
    lines: &[Line<'static>],
    width: usize,
) -> Vec<Option<ratatui::style::Color>> {
    let mut expanded = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        let bg = line_bgs.get(i).copied().flatten();
        let visual_rows = single_line_visual_row_count(line, width);
        for _ in 0..visual_rows {
            expanded.push(bg);
        }
    }
    expanded
}

/// Count how many visual rows a single Line will occupy after wrapping at `width`.
pub(crate) fn single_line_visual_row_count(line: &Line<'_>, width: usize) -> usize {
    if width < 1 {
        return 1;
    }
    Paragraph::new(line.clone())
        .wrap(Wrap::default())
        .line_count(width as u16)
        .max(1)
}

pub(crate) mod bottom_box;
pub(super) mod frame;
mod modals;
mod picker_footer;
mod picker_table;
pub(crate) use picker_footer::PickerFooter;
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
