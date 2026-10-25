use ratatui::{
    Frame,
    layout::{Margin, Rect},
    style::Style,
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::runtime::panels::render_permission_controls;
use crate::{
    runtime::inline_slash_lines_for_render,
    state::{InputMode, PickerRenderKind, PromptStatus},
};

use crate::runtime::RuntimeCoordinator;

impl RuntimeCoordinator {
    pub(crate) fn render_bottom_box(
        &mut self,
        frame: &mut Frame,
        bottom_box_rect: Rect,
        queue_count: u16,
        input_h: u16,
        now_millis: u128,
    ) {
        // ── Frameless bottom stack ─────────────────────────────────────────
        // Stack rows (while queued), a one-row gap, the bordered input box,
        // then the shaded status rows flush below the box.
        let box_border_style = if self.state.scroll.pane_focus == crate::state::PaneFocus::Input {
            self.theme.focus
        } else {
            self.theme.subtle_meta
        };
        frame.render_widget(Clear, bottom_box_rect);
        frame.render_widget(
            Block::default().style(Style::default().bg(self.theme.base)),
            bottom_box_rect,
        );

        // ── Queued-message stack ───────────────────────────────────────────
        if queue_count > 0 {
            let stack_rect = Rect {
                x: bottom_box_rect.x,
                y: bottom_box_rect.y,
                width: bottom_box_rect.width,
                height: queue_count,
            };
            frame.render_widget(
                Block::default().style(Style::default().bg(self.theme.row_user_bg)),
                stack_rect,
            );
            let max_width = bottom_box_rect.width.saturating_sub(2) as usize;
            let queued_lines: Vec<Line> = self
                .state
                .prompt_items()
                .iter()
                .filter(|p| p.status == PromptStatus::Queued)
                .map(|p| {
                    let raw = format!("▏ {}", p.prompt_text);
                    let display = if raw.chars().count() > max_width {
                        format!(
                            "{}…",
                            raw.chars()
                                .take(max_width.saturating_sub(1))
                                .collect::<String>()
                        )
                    } else {
                        raw
                    };
                    let mut chars = display.chars();
                    let rail: String = chars.by_ref().take(2).collect();
                    let text: String = chars.collect();
                    Line::from(vec![
                        Span::styled(rail, self.theme.lane_prefix_user),
                        Span::styled(text, self.theme.role_user),
                    ])
                })
                .collect();
            frame.render_widget(Paragraph::new(Text::from(queued_lines)), stack_rect);
        }

        // ── Input / Permission section ────────────────────────────────────
        let input_top = bottom_box_rect
            .y
            .saturating_add(queue_count)
            .saturating_add(if queue_count > 0 { 1 } else { 0 });
        let input_box_rect = Rect {
            x: bottom_box_rect.x,
            y: input_top,
            width: bottom_box_rect.width,
            height: input_h.saturating_add(2),
        };
        frame.render_widget(
            Block::default()
                .borders(Borders::ALL)
                .border_set(ratatui::symbols::border::ROUNDED)
                .border_style(box_border_style),
            input_box_rect,
        );
        let input_inner = input_box_rect.inner(Margin {
            vertical: 1,
            horizontal: 1,
        });

        // Split input rect: 2 chars for mode indicator, rest for TextArea
        let mode_indicator_width = if input_inner.width >= 3 { 2 } else { 0 };
        let mode_indicator_rect = Rect {
            x: input_inner.x,
            y: input_inner.y,
            width: mode_indicator_width,
            height: input_inner.height,
        };
        let textarea_rect = Rect {
            x: input_inner.x + mode_indicator_width,
            y: input_inner.y,
            width: input_inner.width.saturating_sub(mode_indicator_width),
            height: input_inner.height,
        };

        if self.state.permission.has_prompt() {
            render_permission_controls(frame, input_inner, &self.theme);
        } else {
            // Render mode indicator
            if mode_indicator_width > 0 && textarea_rect.height > 0 {
                let indicator_char = match self.state.input.mode {
                    InputMode::Insert => "❯ ",
                    InputMode::Normal | InputMode::Visual => "❮ ",
                };
                frame.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        indicator_char,
                        self.theme.subtle_meta,
                    ))),
                    mode_indicator_rect,
                );
            }
            // Apply theme styling to TextArea
            self.textarea.set_style(self.theme.input_text);
            // Make TextArea's internal cursor invisible — the terminal cursor
            // (from set_cursor_position) is what the user sees.
            self.textarea
                .set_cursor_style(ratatui::style::Style::default());
            self.textarea
                .set_cursor_line_style(ratatui::style::Style::default());
            // Render TextArea as the input widget
            if textarea_rect.height > 0 {
                frame.render_widget(&self.textarea, textarea_rect);
            }
            // Render inline slash suggestions above TextArea
            if self.state.picker.render_kind() == Some(PickerRenderKind::InlineSlash) {
                let slash_lines = inline_slash_lines_for_render(&self.state, &self.theme);
                if !slash_lines.is_empty() {
                    let content_height = slash_lines.len() as u16;
                    let total_height = content_height + 2;
                    let slash_rect = Rect {
                        x: textarea_rect.x,
                        y: textarea_rect.y.saturating_sub(total_height),
                        width: textarea_rect.width,
                        height: total_height,
                    };
                    frame.render_widget(Clear, slash_rect);
                    frame.render_widget(
                        Block::default().style(Style::default().bg(self.theme.base)),
                        slash_rect,
                    );
                    let block = Block::default()
                        .borders(Borders::ALL)
                        .border_set(ratatui::symbols::border::ROUNDED)
                        .border_style(self.theme.subtle_meta)
                        .title("Commands (up/down - Enter - Esc)");
                    let inner = block.inner(slash_rect);
                    frame.render_widget(block, slash_rect);
                    frame.render_widget(Text::from(slash_lines), inner);
                }
            }
            // Set cursor position in insert mode
            if !self.state.input_locked
                && self.state.picker.active().is_none()
                && self.state.info_panel.is_none()
                && bottom_box_rect.height >= 4
            {
                let sc = self.textarea.screen_cursor();
                // Mirror the crate's internal scroll top (next_scroll_top,
                // crate src/widget.rs:85-93) so the caret's rendered visual row
                // accounts for content scrolled above the input box.
                let prev_top = self.last_input_scroll_top;
                let height = textarea_rect.height;
                let screen_row = sc.row as u16;
                let top_row = if screen_row < prev_top {
                    screen_row
                } else if prev_top + height <= screen_row {
                    screen_row + 1 - height
                } else {
                    prev_top
                };
                self.last_input_scroll_top = top_row;
                let visual_row = screen_row - top_row;
                let x = bottom_box_rect
                    .x
                    .saturating_add(1) // left border
                    .saturating_add(mode_indicator_width)
                    .saturating_add(sc.col as u16);
                let max_x = bottom_box_rect
                    .x
                    .saturating_add(bottom_box_rect.width.saturating_sub(2));
                let y = input_top
                    .saturating_add(1) // top border
                    .saturating_add(visual_row)
                    .min(input_top.saturating_add(input_h)); // clamp to last input row
                frame.set_cursor_position(ratatui::layout::Position { x: x.min(max_x), y });
            }
        }

        // ── Status section ───────────────────────────────────────────────
        let busy_millis = if crate::runtime::status::model_activity_label(&self.state) == "busy" {
            Some(now_millis)
        } else {
            None
        };
        let right_content = crate::runtime::status::status_right_content(
            self.repo_branch_tracker.as_ref().and_then(|t| t.branch()),
            self.repo_branch_tracker
                .as_ref()
                .and_then(|t| t.caller_cwd()),
            &self.theme,
        );
        let right_width = right_content
            .as_ref()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|s| unicode_width::UnicodeWidthStr::width(s.content.as_ref()))
                    .sum::<usize>()
            })
            .unwrap_or(0) as u16;

        let status_top = input_top.saturating_add(input_h).saturating_add(2); // input box bottom border
        let has_message = !self.state.status.message.status_line().is_empty();
        let message_rect = if has_message {
            Some(Rect {
                x: bottom_box_rect.x.saturating_add(1),
                y: status_top,
                width: bottom_box_rect.width.saturating_sub(2),
                height: 1,
            })
        } else {
            None
        };
        let status_inner = Rect {
            x: bottom_box_rect.x.saturating_add(1),
            y: status_top.saturating_add(if has_message { 1 } else { 0 }),
            width: bottom_box_rect.width.saturating_sub(2),
            height: 1,
        };
        let status_inner_2 = Rect {
            y: status_inner.y.saturating_add(1),
            ..status_inner
        };

        let left_width_needed = {
            let probe = crate::runtime::status::status_left_content(
                busy_millis,
                &self.state,
                &self.theme,
                status_inner.width as usize,
            );
            probe
                .spans
                .iter()
                .map(|s| unicode_width::UnicodeWidthStr::width(s.content.as_ref()))
                .sum::<usize>() as u16
        };

        let fits_on_one_line =
            right_width > 0 && left_width_needed + right_width <= status_inner.width;

        if let Some(message_rect) = message_rect
            && let Some(message_line) = crate::runtime::status::status_message_line(
                &self.state,
                &self.theme,
                message_rect.width as usize,
            )
        {
            frame.render_widget(Paragraph::new(message_line), message_rect);
        }

        let left_content = if fits_on_one_line {
            crate::runtime::status::status_left_content(
                busy_millis,
                &self.state,
                &self.theme,
                status_inner.width.saturating_sub(right_width) as usize,
            )
        } else {
            crate::runtime::status::status_left_content(
                busy_millis,
                &self.state,
                &self.theme,
                status_inner.width as usize,
            )
        };

        frame.render_widget(Paragraph::new(left_content), status_inner);

        if let Some(right_line) = right_content {
            if fits_on_one_line {
                frame.render_widget(
                    Paragraph::new(right_line).alignment(ratatui::layout::Alignment::Right),
                    status_inner,
                );
            } else {
                frame.render_widget(
                    Paragraph::new(right_line).alignment(ratatui::layout::Alignment::Right),
                    status_inner_2,
                );
            }
        }
    }
}
