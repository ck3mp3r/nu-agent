use agnt_core::transcript::entry::{MessageEntry, TranscriptEntry};
use agnt_core::transcript::ir::MessageRole;
use agnt_render::backend::RowRect;
use agnt_render::{RenderBackend, Theme};
use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Clear, Paragraph},
};

use crate::rendering::{
    chrome::{Chrome, ContainerBorder},
    theme::TuiTheme,
};
use crate::runtime::panels::{inline_slash_title, render_permission_controls};
use crate::runtime::render::transcript::blit;
use crate::runtime::render::{paint_user_rail_row, render_modal_frame};
use crate::widgets::RatatuiBackend;
use crate::{
    runtime::inline_slash_lines_for_render,
    state::{AppState, CaretGlyph, InputMode, PickerRenderKind, PromptStatus},
};

use crate::runtime::RuntimeCoordinator;

impl RuntimeCoordinator {
    pub(crate) fn render_bottom_box(
        &mut self,
        frame: &mut Frame,
        bottom_box_rect: Rect,
        stack_entries: &[TranscriptEntry],
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
            let mut canvas = Buffer::empty(Rect::new(0, 0, stack_width, stack_h));
            paint_stack_rail_row(&mut canvas, 0, &self.theme);
            let mut row = 1usize;
            for entry in stack_entries {
                row = paint_stack_entry(&mut canvas, row, entry, &self.theme, now_millis);
            }
            paint_stack_rail_row(&mut canvas, row, &self.theme);
            blit(
                frame.buffer_mut(),
                &canvas,
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
        let input_inner = Chrome::default()
            .with_border(ContainerBorder::Rounded)
            .with_border_style(box_border_style)
            .draw(frame, input_box_rect);

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
            // Make TextArea's internal cursor invisible — the caret is a buffer
            // cell painted by this render pass, not a styled cell or a terminal
            // cursor.
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
            // Paint the caret as a buffer cell. The cell belongs to the frame
            // diff: it only changes when the caret moves, so it cannot flicker
            // the way a terminal cursor does under tmux's pane redraw. The
            // terminal cursor stays hidden (no cursor position is set).
            if !self.state.input_locked
                && self.state.picker.active().is_none()
                && self.state.info_panel.is_none()
                && bottom_box_rect.height >= 4
            {
                let sc = self.textarea.screen_cursor();
                // The widget records the display row it drew at the top of the
                // input area, so the caret's visual row is its screen row minus
                // that offset — no scroll arithmetic is mirrored here.
                let visual_row = (sc.row as u16).saturating_sub(self.textarea.scroll_offset().0);
                if let Some(position) =
                    caret_cell_position(textarea_rect, visual_row, sc.col as u16)
                    && let Some(cell) = frame.buffer_mut().cell_mut(position)
                {
                    match self.state.input.mode.caret_glyph() {
                        CaretGlyph::Bar(glyph) => {
                            cell.set_char(glyph);
                            cell.set_style(self.theme.input_text);
                        }
                        // Keep the character under the caret and reverse its
                        // style, so the cell reads as a solid block.
                        CaretGlyph::Underlying => {
                            cell.set_style(self.theme.input_text.add_modifier(Modifier::REVERSED));
                        }
                    }
                }
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

/// Paint one queued prompt's rows through the message widget, one buffer row
/// below the stack's top padding. Returns the buffer row after the entry.
fn paint_stack_entry(
    canvas: &mut Buffer,
    row: usize,
    entry: &TranscriptEntry,
    theme: &TuiTheme,
    now_millis: u128,
) -> usize {
    let width = usize::from(canvas.area.width);
    let probe = RatatuiBackend::new(canvas.area.width, 0);
    let rows = probe.measure(entry, width);
    if rows == 0 {
        return row;
    }
    let Ok(height) = u16::try_from(rows) else {
        return row;
    };
    let Ok(top) = u16::try_from(row) else {
        return row;
    };
    let mut backend = RatatuiBackend::new(canvas.area.width, height);
    backend.set_now_millis(now_millis);
    backend.render(entry, RowRect::new(0, width, rows), &Theme::from(theme));
    let rect = Rect {
        x: 0,
        y: top,
        width: canvas.area.width,
        height,
    };
    blit(canvas, backend.buffer(), rect);
    row.saturating_add(rows)
}

/// Paint the stack's padding row: the user rail, so the box's top and bottom
/// rows keep the gutter continuous down the whole block.
fn paint_stack_rail_row(canvas: &mut Buffer, row: usize, theme: &TuiTheme) {
    let Ok(y) = u16::try_from(row) else {
        return;
    };
    paint_user_rail_row(canvas, y, usize::from(canvas.area.width), theme);
}

/// The buffer cell the input caret paints, or `None` when the caret's row falls
/// outside the textarea rect (the widget is scrolled past that row).
///
/// `caret_row` is the caret's visual row inside the rect — the widget's screen
/// cursor row minus its scroll offset — and `caret_col` its column. A column
/// past the right edge clamps to the rect's last column, so the caret never
/// spills over the border.
pub(crate) fn caret_cell_position(
    textarea_rect: Rect,
    caret_row: u16,
    caret_col: u16,
) -> Option<Position> {
    let position = Position {
        x: textarea_rect
            .x
            .saturating_add(caret_col)
            .min(textarea_rect.right().saturating_sub(1)),
        y: textarea_rect.y.saturating_add(caret_row),
    };
    textarea_rect.contains(position).then_some(position)
}

/// One user-message entry per queued prompt, in queue order. Each entry is the
/// same shape a completed user turn pushes into the transcript, so the stack
/// renders through the message widget instead of a private style path.
pub(crate) fn queued_prompt_entries(state: &AppState) -> Vec<TranscriptEntry> {
    state
        .prompt_items()
        .iter()
        .filter(|prompt| prompt.status == PromptStatus::Queued)
        .map(|prompt| {
            TranscriptEntry::Message(MessageEntry {
                role: MessageRole::User,
                markdown: prompt.prompt_text.clone(),
            })
        })
        .collect()
}

/// Rows the stack occupies at `width`: the two tinted padding rows (one above
/// the text, one below) plus every queued prompt's wrapped rows. `0` when
/// nothing is queued, so an empty queue adds no stack row and no gap row. The
/// backend measures, so the reserved height and the painted rows agree by
/// construction.
pub(crate) fn stacked_messages_height(entries: &[TranscriptEntry], width: usize) -> u16 {
    if entries.is_empty() {
        return 0;
    }
    let probe = RatatuiBackend::new(u16::try_from(width).unwrap_or(u16::MAX), 0);
    let rows = entries
        .iter()
        .map(|entry| probe.measure(entry, width))
        .sum::<usize>()
        + 2;
    u16::try_from(rows).unwrap_or(u16::MAX)
}

// endregion: --- Support
