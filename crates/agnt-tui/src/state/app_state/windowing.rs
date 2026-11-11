//! The offscreen transcript windowing the yank payload is built from.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::runtime::render::transcript::{PaintWindow, paint_transcript_rows};

use super::AppState;

impl AppState {
    /// Renders the transcript rows `start_row..=end_row` offscreen and returns
    /// them as newline-joined text.
    pub(super) fn selection_text(&mut self, start_row: usize, end_row: usize) -> String {
        let width = self.scroll.transcript_area.width.saturating_sub(2) as usize;
        if width == 0 {
            return String::new();
        }
        if !self.transcript.height_index_valid_for(width) {
            self.transcript.rebuild_height_index(width);
        }
        let total_rows = self.transcript.total_visual_rows();
        if start_row >= total_rows {
            return String::new();
        }
        let end_row = end_row.min(total_rows.saturating_sub(1));
        let rows = end_row - start_row + 1;
        let area = Rect::new(
            0,
            0,
            width.min(u16::MAX as usize) as u16,
            rows.min(u16::MAX as usize) as u16,
        );
        let mut buffer = Buffer::empty(area);
        let window = PaintWindow {
            theme: &self.theme,
            width,
            first_row: start_row,
            height: rows,
            top: 0,
        };
        paint_transcript_rows(&self.transcript, &window, &mut buffer);
        let mut text_rows: Vec<String> = Vec::with_capacity(rows);
        for y in 0..area.height {
            let mut text = String::new();
            for x in 0..area.width {
                if let Some(cell) = buffer.cell((x, y)) {
                    text.push(cell.symbol().chars().next().unwrap_or(' '));
                }
            }
            text_rows.push(text.trim_end().to_string());
        }
        text_rows.join("\n")
    }
}
