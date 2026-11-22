//! The coordinator's paint path: the frame kinds, the synchronized-output block,
//! the composed frame, the selection highlight, and the frame-sync error.

use super::RuntimeCoordinator;
use crate::rendering::chrome::{Chrome, ContainerBorder};
use crate::rendering::layout::MAIN_SIDE_MARGIN;
use crate::runtime::backend::FrameSync;
use crate::runtime::frame_timing::FrameKind;
use crate::runtime::layout::{STATS_BAND_ROWS, compute_bottom_box_height, compute_status_h};
use crate::runtime::render::bottom_box::{queued_prompt_entries, stacked_messages_height};
use crate::runtime::render::clock_cells::{ClockCell, PaintedFrame};
use crate::runtime::render::transcript::PaintWindow;
use crate::runtime::status::{status_left_content, status_right_content};
use crate::state::PickerRenderKind;
use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph};
use std::time::Instant;

impl RuntimeCoordinator {
    /// Apply the selection highlight to the buffer rows that host the selection's
    /// visual rows.
    ///
    /// The rows come from the same [`PaintWindow`] the paint used, so the
    /// highlight lands where the paint put those rows — the window's `top`
    /// padding included. The visible frame and the highlight cannot drift.
    pub(crate) fn apply_selection_highlight(
        buffer: &mut ratatui::buffer::Buffer,
        area: ratatui::layout::Rect,
        sel_start: usize,
        sel_end: usize,
        window: &PaintWindow<'_>,
        selection_style: ratatui::style::Style,
    ) {
        for vis_row in sel_start..=sel_end {
            if !window.contains(vis_row) {
                continue;
            }
            let Ok(buffer_row) = u16::try_from(window.buffer_row(vis_row)) else {
                continue;
            };
            let row_screen_y = area.y.saturating_add(buffer_row);
            for x in area.x..area.x + area.width {
                if let Some(cell) =
                    buffer.cell_mut(ratatui::layout::Position { x, y: row_screen_y })
                {
                    cell.set_style(selection_style);
                }
            }
        }
    }

    /// Paint the frame `kind` asks for, inside one synchronized-output block.
    ///
    /// Both kinds share the block's prologue and epilogue: the cursor shape and
    /// the block itself are frame-level writes, so they belong to the frame
    /// whichever way its body is painted.
    pub(super) fn paint<B: ratatui::backend::Backend + FrameSync>(
        &mut self,
        live: &mut Option<&mut ratatui::Terminal<B>>,
        kind: FrameKind,
    ) -> Result<(), String> {
        let Some(live) = live.as_mut() else {
            return Ok(());
        };
        let frame_started = Instant::now();
        let cursor_style = self.state.input.mode.cursor_style();

        // A synchronized-output block must never outlive its frame: the terminal
        // would keep buffering and stop repainting. So open the block, run every
        // phase, and only then report the first failure.
        let begin_sync = live.backend_mut().begin_sync().map_err(frame_sync_error);

        // Values the FnOnce draw closure smuggles back out: the bytes the frame
        // left on the surface, the resolved scroll offset, the cells a clock
        // step repaints, and whether this frame was the clock-only repaint.
        let mut painted_buffer: Option<ratatui::buffer::Buffer> = None;
        let mut rendered_scroll_offset: Option<usize> = None;
        let mut clock_cells: Vec<ClockCell> = Vec::new();
        let mut clock_only = false;

        let draw = live
            .draw(|frame| {
                clock_only = match kind {
                    FrameKind::ClockCells => self.repaint_clock_cells(frame),
                    FrameKind::Recomposed => false,
                };
                if !clock_only {
                    self.paint_recomposed(frame, &mut rendered_scroll_offset, &mut clock_cells);
                }
            })
            .map(|completed| painted_buffer = Some(completed.buffer.clone()))
            .map_err(|err| format!("TUI render failed: {err}"));
        let set_cursor_style = live
            .backend_mut()
            .set_cursor_style(cursor_style)
            .map_err(frame_sync_error);
        let end_sync = live.backend_mut().end_sync().map_err(frame_sync_error);

        begin_sync?;
        draw?;

        // Write back the resolved scroll offset so that scroll_offset always
        // reflects the actual rendered position. Without this, when following_tail is true
        // scroll_offset stays at 0 and the first scroll-up key jumps to the top.
        if let Some(offset) = rendered_scroll_offset {
            self.state.scroll.scroll_offset = offset;
        }

        // The snapshot a clock-only frame patches. Its clock cells stay when the
        // frame was one: the next step reads the same cells.
        if let Some(buffer) = painted_buffer {
            match self.last_painted.as_mut() {
                Some(painted) if clock_only => painted.replace_buffer(buffer),
                _ => self.last_painted = Some(PaintedFrame::new(buffer, clock_cells)),
            }
        }

        set_cursor_style?;
        end_sync?;
        let painted_kind = if clock_only {
            FrameKind::ClockCells
        } else {
            FrameKind::Recomposed
        };
        let frame_elapsed = frame_started.elapsed();
        if self
            .frame_timing
            .record_frame(frame_elapsed, painted_kind, self.frame_clock_millis)
        {
            let timing = self.frame_timing();
            log::warn!(
                "tui-frame-slow: frame paint took {} ms; {} of {} painted frames slow ({} recomposed, {} clock-only); {} input re-wraps; row cache: {} hits, {} misses, {} parked entries ({} rows), {} evicted, {} invalidations",
                timing.last_frame_millis(),
                timing.slow_frames(),
                timing.frames_painted(),
                timing.recomposed_frames(),
                timing.clock_frames(),
                self.layout_recomputes(),
                self.row_cache.hits(),
                self.row_cache.misses(),
                self.row_cache.parked_len(),
                self.row_cache.parked_rows(),
                self.row_cache.evicted_total(),
                self.row_cache.invalidations()
            );
        }
        Ok(())
    }

    /// Compose every region of the frame from the state, and collect the cells a
    /// clock step repaints.
    fn paint_recomposed(
        &mut self,
        frame: &mut ratatui::Frame,
        rendered_scroll_offset: &mut Option<usize>,
        clock_cells: &mut Vec<ClockCell>,
    ) {
        // Capture values needed inside the body (avoid borrowing self mutably)
        let transcript_following_tail = self.state.scroll.following_tail;
        let transcript_scroll_offset = self.state.scroll.scroll_offset;

        let area = frame.area();
        frame.render_widget(
            Block::default().style(Style::default().bg(self.theme.base)),
            area,
        );
        let has_side = self.side_pane_visible.unwrap_or(false);
        let horizontal = if has_side {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
                .split(area)
        } else {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(100)])
                .split(area)
        };

        let main = horizontal[0];
        let side_margin = if main.width >= 8 { MAIN_SIDE_MARGIN } else { 0 };
        let content_main = main.inner(Margin {
            vertical: 0,
            horizontal: side_margin,
        });
        self.last_input_content_width = content_main.width.saturating_sub(4) as usize;
        let available_inner_w = content_main.width.saturating_sub(2) as usize;
        let pre_right_width = {
            let rc = status_right_content(
                self.repo_branch(),
                self.repo_branch_tracker
                    .as_ref()
                    .and_then(|t| t.caller_cwd()),
                &self.theme,
            );
            rc.map(|line| {
                line.spans
                    .iter()
                    .map(|s| unicode_width::UnicodeWidthStr::width(s.content.as_ref()))
                    .sum::<usize>()
            })
            .unwrap_or(0)
        };
        let pre_left_width = {
            let probe = status_left_content(None, &self.state, &self.theme, available_inner_w);
            probe
                .spans
                .iter()
                .map(|s| unicode_width::UnicodeWidthStr::width(s.content.as_ref()))
                .sum::<usize>()
        };
        let has_message = !self.state.status.message.status_line().is_empty();
        let status_h = compute_status_h(
            available_inner_w,
            pre_left_width,
            pre_right_width,
            has_message,
        );
        let stack_entries = queued_prompt_entries(&self.state);
        let stack_h = stacked_messages_height(&stack_entries, available_inner_w);
        let bottom_box_h = compute_bottom_box_height(
            stack_h,
            self.input_height,
            status_h,
            u16::from(self.state.render_stats_visible).saturating_mul(STATS_BAND_ROWS),
        );
        let vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(0),
                Constraint::Fill(1),
                Constraint::Length(bottom_box_h),
            ])
            .split(content_main);
        // vertical[0]=unused [1]=transcript [2]=entire bottom box

        let transcript_content_area = vertical[1];
        self.state.scroll.transcript_area = transcript_content_area;
        let transcript_list_area = Rect {
            width: transcript_content_area.width.saturating_sub(2),
            ..transcript_content_area
        };

        if vertical[1].height > 0 {
            clock_cells.extend(self.render_transcript_pane(
                frame,
                transcript_content_area,
                transcript_list_area,
                transcript_following_tail,
                transcript_scroll_offset,
                rendered_scroll_offset,
            ));
        }

        let now_millis = self.frame_clock_millis;
        clock_cells.extend(self.render_bottom_box(
            frame,
            vertical[2],
            &stack_entries,
            stack_h,
            self.input_height,
            now_millis,
        ));

        if has_side {
            let side = horizontal[1];
            let pane_style = Style::default().fg(self.theme.row_assistant.fg.unwrap_or_default());
            // The pane's foreground covers the whole rect, border included,
            // so paint it before the chrome lays the rules down.
            frame.render_widget(Block::default().style(pane_style), side);
            let side_inner = Chrome::default()
                .with_border(ContainerBorder::Plain)
                .with_title("Events")
                .draw(frame, side);
            frame.render_widget(
                Paragraph::new(Line::from("Events pane reserved")).style(pane_style),
                side_inner,
            );
        }

        match self.state.picker.render_kind() {
            Some(PickerRenderKind::CommandPalette) => {
                self.render_command_palette(frame, transcript_content_area);
            }
            Some(PickerRenderKind::Model) => {
                self.render_model_picker(frame, transcript_content_area)
            }
            Some(PickerRenderKind::Agent) => {
                self.render_agent_picker(frame, transcript_content_area)
            }
            Some(PickerRenderKind::Session) => {
                self.render_session_picker(frame, transcript_content_area)
            }
            Some(PickerRenderKind::Theme) => {
                self.render_theme_picker(frame, transcript_content_area)
            }
            Some(PickerRenderKind::InlineSlash) | None => {}
        }

        if self.state.info_panel.is_some() {
            self.render_info_panel(frame, transcript_content_area);
        }
    }
}

/// Map a frame-control write failure to the render path's error string.
fn frame_sync_error(err: std::io::Error) -> String {
    format!("TUI frame sync failed: {err}")
}
