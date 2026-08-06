use ratatui::{
    Frame,
    layout::{Margin, Rect},
    text::{Line, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

pub(super) fn render_scroll_text_panel(
    frame: &mut Frame,
    area: Rect,
    title: impl Into<Line<'static>>,
    lines: Text<'_>,
    scroll: usize,
) {
    let inner = render_modal_frame(frame, area, title);
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
) -> Rect {
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_set(ratatui::symbols::border::ROUNDED)
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
pub(super) fn single_line_visual_row_count(line: &Line<'_>, width: usize) -> usize {
    if width < 1 {
        return 1;
    }
    let text = Paragraph::new(ratatui::text::Text::from(line.clone())).wrap(Wrap::default());
    text.line_count(width as u16).max(1)
}

mod bottom_box;
mod modals;
mod transcript;
