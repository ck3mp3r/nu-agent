use agnt_core::transcript::ir::{Block as IrBlock, MessageRole};
use agnt_core::transcript::items::Message;
use agnt_core::transcript::renderer::{FrameContext, Renderable};
use ratatui::{
    Frame,
    layout::{Margin, Rect},
    style::Style,
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::rendering::theme::TuiTheme;
use crate::runtime::panels::{inline_slash_title, render_permission_controls};
use crate::runtime::render::{render_modal_frame, user_rail_line};
use crate::tui_renderer::{layout, measure};
use crate::{
    runtime::inline_slash_lines_for_render,
    state::{AppState, InputMode, PickerRenderKind, PromptStatus},
};

use crate::runtime::RuntimeCoordinator;

impl RuntimeCoordinator {
    pub(crate) fn render_bottom_box(
        &mut self,
        frame: &mut Frame,
        bottom_box_rect: Rect,
        stack_blocks: &[IrBlock],
        stack_h: u16,
        input_h: u16,
        now_millis: u128,
    ) {
        // ── Frameless bottom stack ─────────────────────────────────────────
        // A one-row gap (while queued), the stacked-message rows flush above
        // the bordered input box, then the shaded status rows flush below it.
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
        // The gap row sits above the stack so the stack separates from the
        // transcript and hugs the input box. The stack renders through the
        // transcript's user-lane pipeline, so a queued prompt looks like the
        // user message it becomes. The whole stack is ONE merged block: one
        // tinted padding row, every queued prompt's rows back to back (the
        // user rail continues down the block, so the gutter never breaks),
        // then one tinted padding row. Both padding rows sit inside the same
        // background box, so they carry the rail too (task 4d99956c).
        let stack_gap = u16::from(stack_h > 0);
        let stack_top = bottom_box_rect.y.saturating_add(stack_gap);
        let stack_width = bottom_box_rect.width.saturating_sub(2);
        if stack_h > 0 {
            let stack_rect = Rect {
                x: bottom_box_rect.x,
                y: stack_top,
                width: bottom_box_rect.width,
                height: stack_h,
            };
            frame.render_widget(
                Block::default().style(Style::default().bg(self.theme.row_user_bg)),
                stack_rect,
            );
            let ctx = FrameContext {
                width: stack_width as usize,
                now_millis,
                cursor: false,
                selected: false,
            };
            let mut lines: Vec<Line<'static>> = Vec::with_capacity(stack_h as usize);
            lines.push(user_rail_line(&self.theme));
            for block in stack_blocks {
                lines.extend(layout(block, &ctx, &self.theme));
            }
            lines.push(user_rail_line(&self.theme));
            frame.render_widget(
                Paragraph::new(Text::from(lines)),
                Rect {
                    width: stack_width,
                    ..stack_rect
                },
            );
        }

        // ── Input / Permission section ────────────────────────────────────
        let input_top = bottom_box_rect
            .y
            .saturating_add(stack_gap)
            .saturating_add(stack_h);
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
                    let inner =
                        render_modal_frame(frame, slash_rect, inline_slash_title(), &self.theme);
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

// region:    --- Support

/// One user-lane block per queued prompt, in queue order. Each block is the
/// same IR a completed user turn pushes into the transcript, so the stack
/// renders through the transcript's user-message pipeline instead of a
/// private style path.
pub(crate) fn queued_prompt_blocks(state: &AppState) -> Vec<IrBlock> {
    state
        .prompt_items()
        .iter()
        .filter(|prompt| prompt.status == PromptStatus::Queued)
        .map(|prompt| {
            let message = Message {
                role: MessageRole::User,
                markdown: prompt.prompt_text.clone(),
            };
            IrBlock {
                source: message.source(),
                lane: message.lane(),
                fill: message.fill(),
                status: None,
            }
        })
        .collect()
}

/// Rows the stack occupies at `width`: the two tinted padding rows (one above
/// the text, one below) plus every queued prompt's wrapped rows. `0` when
/// nothing is queued, so an empty queue adds no stack row and no gap row.
/// [`measure`] is the row-count twin of the [`layout`] call that paints the
/// stack, so the reserved height and the rendered rows agree by construction.
pub(crate) fn stacked_messages_height(blocks: &[IrBlock], width: usize, theme: &TuiTheme) -> u16 {
    if blocks.is_empty() {
        return 0;
    }
    let rows = blocks
        .iter()
        .map(|block| measure(block, width, theme))
        .sum::<usize>()
        + 2;
    u16::try_from(rows).unwrap_or(u16::MAX)
}

// endregion: --- Support
