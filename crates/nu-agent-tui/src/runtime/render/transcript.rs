use ratatui::{
    Frame,
    layout::Rect,
    text::Line,
    widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
};

use crate::runtime::{
    transcript_entries_for_render, transcript_line_status_to_item_status,
    transcript_line_statuses_for_render,
};
use crate::tui_renderer::TuiRenderer;
use crate::{
    runtime::{render::expand_to_visual_rows, render_frame::current_time_millis},
    state::{InputMode, PaneFocus},
};
use nu_agent_core::transcript::items::Renderable;
use nu_agent_core::transcript::renderer::{BlockRenderer, RenderContext};

use crate::runtime::RuntimeCoordinator;

impl RuntimeCoordinator {
    pub(crate) fn render_transcript_pane(
        &mut self,
        frame: &mut Frame,
        transcript_content_area: Rect,
        transcript_list_area: Rect,
        transcript_following_tail: bool,
        transcript_scroll_offset: usize,
        rendered_scroll_offset: &mut Option<usize>,
    ) {
        let now_millis = current_time_millis();
        let renderer = TuiRenderer {
            theme: self.theme.clone(),
        };

        frame.render_widget(ratatui::widgets::Clear, transcript_content_area);
        if transcript_content_area.height > 0 {
            let width = transcript_list_area.width as usize;

            let entries_for_render = transcript_entries_for_render(&self.state);
            let transcript_line_statuses =
                transcript_line_statuses_for_render(&self.state, entries_for_render);

            let mut all_lines: Vec<Line<'static>> = Vec::new();
            let mut entry_indices: Vec<usize> = Vec::new();

            for (idx, entry) in entries_for_render.iter().enumerate() {
                let block = entry.to_render_block();
                let item_status = transcript_line_statuses
                    .get(idx)
                    .copied()
                    .flatten()
                    .map(transcript_line_status_to_item_status);
                let ctx = RenderContext {
                    width,
                    cursor: false,
                    selected: false,
                    status: item_status,
                    now_millis,
                };
                let entry_lines = renderer.render(&block, &ctx);
                for _ in 0..entry_lines.len() {
                    entry_indices.push(idx);
                }
                all_lines.extend(entry_lines);
            }

            // Expand entry_indices to visual rows for cursor/selection mapping
            let expanded_entry_indices =
                expand_to_visual_rows(entry_indices.clone(), &all_lines, width);
            self.state.entry_indices = expanded_entry_indices.clone();

            let text = ratatui::text::Text::from(all_lines);
            let paragraph = Paragraph::new(text).wrap(Wrap::default());
            let total_visual_rows = paragraph.line_count(transcript_list_area.width);

            let viewport_height = transcript_list_area.height as usize;
            self.state.viewport_height = viewport_height;
            self.state.total_visual_rows = total_visual_rows;
            let max_scroll: usize = total_visual_rows.saturating_sub(viewport_height);
            self.state.max_scroll = max_scroll;
            let effective_offset: usize = if transcript_following_tail {
                max_scroll
            } else {
                transcript_scroll_offset.min(max_scroll)
            };
            *rendered_scroll_offset = Some(effective_offset);
            // When following tail, keep cursor at the last visual row
            if transcript_following_tail {
                self.state.cursor_visual_row = total_visual_rows.saturating_sub(1);
            }
            // When not tailing, cursor_visual_row is user-controlled — don't touch it

            let scroll_u16 = effective_offset.min(u16::MAX as usize) as u16;
            frame.render_widget(paragraph.scroll((scroll_u16, 0)), transcript_list_area);

            // Store rendered text per visible viewport row for yank support
            let mut rendered_text: Vec<String> = Vec::with_capacity(viewport_height);
            for row in 0..viewport_height {
                let row_screen_y = transcript_list_area.y + row as u16;
                let mut row_text = String::new();
                for x in transcript_list_area.x..transcript_list_area.x + transcript_list_area.width
                {
                    if let Some(cell) = frame.buffer_mut().cell((x, row_screen_y)) {
                        let ch = cell.symbol().chars().next().unwrap_or(' ');
                        row_text.push(ch);
                    }
                }
                rendered_text.push(row_text.trim_end().to_string());
            }
            self.state.rendered_line_text = rendered_text;
            self.state.rendered_line_start_row = effective_offset;

            // Post-render buffer manipulation: apply selection highlight to visual rows
            if self.state.input_mode == InputMode::Visual
                && let Some(ref sel) = self.state.transcript_selection
            {
                let (sel_start, sel_end) = sel.normalized_range();
                Self::apply_selection_highlight(
                    frame.buffer_mut(),
                    transcript_list_area,
                    sel_start,
                    sel_end,
                    effective_offset,
                    viewport_height,
                );
            }

            // Overlay > cursor indicator at the correct screen position
            if self.state.pane_focus == PaneFocus::Transcript
                && self.state.cursor_visual_row >= effective_offset
                && self.state.cursor_visual_row < effective_offset + viewport_height
                && (self.state.input_mode == InputMode::Normal
                    || self.state.input_mode == InputMode::Visual)
            {
                let cursor_y = (self.state.cursor_visual_row - effective_offset) as u16;
                let cursor_screen_y = transcript_list_area.y + cursor_y;
                frame.render_widget(
                    Paragraph::new("> "),
                    Rect::new(transcript_list_area.x, cursor_screen_y, 2, 1),
                );
            }

            if total_visual_rows > viewport_height {
                let mut scrollbar_state =
                    ScrollbarState::new(max_scroll).position(effective_offset);
                frame.render_stateful_widget(
                    Scrollbar::new(ScrollbarOrientation::VerticalRight)
                        .begin_symbol(None)
                        .end_symbol(None),
                    transcript_content_area,
                    &mut scrollbar_state,
                );
            }
        }
    }
}
