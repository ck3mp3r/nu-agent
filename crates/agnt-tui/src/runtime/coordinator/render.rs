//! The coordinator's frame policy: what raises a frame, and when the loop paints
//! one.

use std::time::Instant;

use super::RuntimeCoordinator;
use crate::runtime::backend::FrameSync;
use crate::runtime::frame_timing::FrameKind;
use crate::runtime::render::frame::current_time_millis;

impl RuntimeCoordinator {
    /// Raise a full frame: the state changed, so every region is composed
    /// again. A pending clock-only frame never survives a state change.
    pub(crate) fn mark_render_needed(&mut self) {
        self.pending_frame = Some(FrameKind::Recomposed);
    }

    /// Raise a clock-only frame: the spinner step advanced and nothing else
    /// changed, so only the cells that read the frame clock are repainted. A
    /// frame already pending stays as it is — a recomposition covers the clock
    /// cells too, and a state change never downgrades to a repaint.
    ///
    /// The render-stats row carries the loop's own per-frame counters, so a
    /// visible row changes with every frame: it must be composed again, which
    /// makes the tick a full frame.
    pub(crate) fn mark_clock_tick(&mut self) {
        if self.state.render_stats_visible {
            self.mark_render_needed();
            return;
        }
        self.pending_frame = self.pending_frame.or(Some(FrameKind::ClockCells));
    }

    pub(crate) fn render_if_needed<B: ratatui::backend::Backend + FrameSync>(
        &mut self,
        live: &mut Option<&mut ratatui::Terminal<B>>,
    ) -> Result<(), String> {
        let Some(kind) = self.pending_frame else {
            return Ok(());
        };
        if self.last_render_at.elapsed() < Self::MIN_FRAME_INTERVAL {
            return Ok(());
        }
        self.pending_frame = None;
        self.last_render_at = Instant::now();
        self.theme = self.state.theme.clone();
        // One reading per frame: every region of the frame names the same time.
        self.frame_clock_millis = current_time_millis();
        match kind {
            FrameKind::Recomposed => self.render_frame(live),
            FrameKind::ClockCells => self.render_clock_frame(live),
        }
    }

    /// Compose the whole frame from the state, at the stored frame clock.
    pub(crate) fn render_frame<B: ratatui::backend::Backend + FrameSync>(
        &mut self,
        live: &mut Option<&mut ratatui::Terminal<B>>,
    ) -> Result<(), String> {
        self.paint(live, FrameKind::Recomposed)
    }

    /// Repaint the cells that read the frame clock over the last painted frame,
    /// at the stored frame clock.
    pub(crate) fn render_clock_frame<B: ratatui::backend::Backend + FrameSync>(
        &mut self,
        live: &mut Option<&mut ratatui::Terminal<B>>,
    ) -> Result<(), String> {
        self.paint(live, FrameKind::ClockCells)
    }
}
