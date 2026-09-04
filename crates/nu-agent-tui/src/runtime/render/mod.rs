use ratatui::{
    Frame,
    layout::{Margin, Rect},
    text::{Line, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::rendering::theme::TuiTheme;
use crate::state::InputMode;

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
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((scroll.min(u16::MAX as usize) as u16, 0)),
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

mod bottom_box;
pub(super) mod frame;
mod modals;
mod transcript;

#[cfg(test)]
pub(super) mod frame_test;

#[cfg(test)]
pub(super) mod selection_render_test;

#[cfg(test)]
mod transcript_test;

/// Returns true when the buffer should be scanned for yank text.
/// Only needed in Visual mode; avoids per-frame O(width*height) cost otherwise.
pub(crate) fn should_scan_for_yank(mode: InputMode) -> bool {
    mode == InputMode::Visual
}
