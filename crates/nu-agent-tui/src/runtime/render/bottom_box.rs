use ratatui::{
    Frame,
    layout::{Margin, Rect},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::runtime::panels::render_permission_controls;
use crate::{
    runtime::inline_slash_lines_for_render,
    state::{InputMode, PromptStatus},
};

use crate::runtime::RuntimeCoordinator;

impl RuntimeCoordinator {
    pub(crate) fn render_bottom_box(
        &mut self,
        frame: &mut Frame,
        vertical: &[Rect],
        now_millis: u128,
    ) {
        // ── Unified bottom box ──────────────────────────────────────────────
        // Combine queue (vertical[2]), input (vertical[3]), and status
        // (vertical[4]) into one rounded box with ├─┤ dividers.
        let bottom_box_rect = Rect {
            x: vertical[2].x,
            y: vertical[2].y,
            width: vertical[2].width,
            height: vertical[2]
                .height
                .saturating_add(vertical[3].height)
                .saturating_add(vertical[4].height),
        };

        let box_border_style = if self.state.pane_focus == crate::state::PaneFocus::Input {
            self.theme.focus
        } else {
            self.theme.subtle_meta
        };
        frame.render_widget(Clear, bottom_box_rect);
        frame.render_widget(
            Block::default()
                .borders(Borders::ALL)
                .border_set(ratatui::symbols::border::ROUNDED)
                .border_style(box_border_style),
            bottom_box_rect,
        );
        let inner = bottom_box_rect.inner(Margin {
            vertical: 1,
            horizontal: 1,
        });

        // Helper: draw ├────────────────┤ divider at absolute y
        let draw_divider = |frame: &mut ratatui::Frame, div_y: u16| {
            let divider_line = Line::from(vec![
                Span::styled("├", box_border_style),
                Span::styled("─".repeat(inner.width as usize), box_border_style),
                Span::styled("┤", box_border_style),
            ]);
            frame.render_widget(
                Paragraph::new(divider_line),
                Rect {
                    x: bottom_box_rect.x,
                    y: div_y,
                    width: bottom_box_rect.width,
                    height: 1,
                },
            );
        };

        // ── Queue section ────────────────────────────────────────────────
        let queue_count = self.state.pending_prompt_count() as u16;
        if vertical[2].height > 0 {
            let queue_inner = Rect {
                x: inner.x,
                y: inner.y,
                width: inner.width,
                height: vertical[2].height,
            };
            let pane_width = inner.width as usize;
            let queued_lines: Vec<Line> = self
                .state
                .prompt_items()
                .iter()
                .filter(|p| p.status == PromptStatus::Queued)
                .flat_map(|p| {
                    let raw = format!("• {}", p.prompt_text);
                    let display = if raw.chars().count() > pane_width {
                        format!(
                            "{}…",
                            raw.chars()
                                .take(pane_width.saturating_sub(1))
                                .collect::<String>()
                        )
                    } else {
                        raw
                    };
                    [Line::from(Span::styled(display, self.theme.role_user))]
                })
                .collect();
            frame.render_widget(Paragraph::new(Text::from(queued_lines)), queue_inner);

            // Divider after queue — drawn at the last row of the queue
            // region (inner.y + queue_count), so input starts one row
            // below the divider instead of on top of it.
            let div_y = inner.y.saturating_add(queue_count);
            draw_divider(frame, div_y);
        }

        // ── Input / Permission section ────────────────────────────────────
        let input_inner_h = vertical[3].height.saturating_sub(2);
        let input_inner = Rect {
            x: inner.x,
            y: inner.y.saturating_add(vertical[2].height),
            width: inner.width,
            height: input_inner_h,
        };

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

        // Divider after input
        let input_div_y = bottom_box_rect
            .y
            .saturating_add(1)
            .saturating_add(vertical[2].height)
            .saturating_add(input_inner_h);
        draw_divider(frame, input_div_y);

        if self.state.permission_prompt.is_some() {
            render_permission_controls(frame, input_inner, &self.theme);
        } else {
            // Render mode indicator
            if mode_indicator_width > 0 && textarea_rect.height > 0 {
                let indicator_char = match self.state.input_mode {
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
            if self.state.inline_slash_open {
                let slash_lines = inline_slash_lines_for_render(&self.state);
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
                && !self.state.command_palette_open
                && self.state.info_panel.is_none()
                && bottom_box_rect.height >= 4
            {
                let ratatui_textarea::DataCursor(row, col) = self.textarea.cursor();
                let x = bottom_box_rect
                    .x
                    .saturating_add(1) // left border
                    .saturating_add(mode_indicator_width)
                    .saturating_add(col as u16);
                let max_x = bottom_box_rect
                    .x
                    .saturating_add(bottom_box_rect.width.saturating_sub(2));
                let y = bottom_box_rect
                    .y
                    .saturating_add(1) // top border
                    .saturating_add(vertical[2].height) // queue section
                    .saturating_add(row as u16)
                    .min(input_div_y.saturating_sub(1)); // clamp to last input row
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

        let status_inner = Rect {
            x: inner.x.saturating_add(1),
            y: input_div_y.saturating_add(1),
            width: inner.width.saturating_sub(2),
            height: 1,
        };
        let status_inner_2 = Rect {
            y: status_inner.y.saturating_add(1),
            ..status_inner
        };

        let left_width_needed = {
            let probe = crate::runtime::status::status_left_content(
                &self.active_model_identity,
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

        let left_content = if fits_on_one_line {
            crate::runtime::status::status_left_content(
                &self.active_model_identity,
                busy_millis,
                &self.state,
                &self.theme,
                status_inner.width.saturating_sub(right_width) as usize,
            )
        } else {
            crate::runtime::status::status_left_content(
                &self.active_model_identity,
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
