//! The task family widget: a background task's call line.
//!
//! One task entry for the task's whole life. The line renders through the same
//! [`CallLineRow`] the tool family uses — the status indicator, the task's name
//! in emphasis, and its subject in muted — and carries no display body: what the
//! task produces (a compaction's streamed summary) renders below it as its own
//! entries. The line is pushed once and never rewritten; only the glyph moves.

use agnt_render::catalog::{CallLineRow, LaneKind, LaneLine};
use agnt_render::transcript::entry::TaskEntry;
use agnt_render::transcript::ir::ContentLine;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::components::{lane_marker_style, lane_text_style, status_style};
use super::text::{RowPaint, VisualRow, paint_row, wrap_line};
use super::{PaintCtx, PaneWidget};

// region:    --- Types

/// One task call line: the header row only.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TaskWidget<'a> {
    pub entry: &'a TaskEntry,
}

// endregion: --- Types

// region:    --- Support

impl TaskWidget<'_> {
    fn lane(&self) -> LaneLine {
        LaneLine::new(LaneKind::Compaction)
    }

    /// The row the call line renders: the task's name, its subject after the
    /// `·` separator, and the live status.
    fn call_line(&self) -> CallLineRow {
        CallLineRow::new(
            self.entry.name.clone(),
            format!("· {}", self.entry.summary),
            Some(self.entry.status),
        )
    }

    /// The call line's wrap budget: the status indicator takes its columns on
    /// row 0, so every call row wraps short of it — the tool lane's rule.
    fn call_rows(&self, width: usize) -> Vec<VisualRow> {
        let content = ContentLine::from_spans(self.call_line().spans());
        let budget =
            crate::state::code_block::content_wrap_width(width, true, self.lane().prefix_width());
        wrap_line(&content, budget)
    }
}

// endregion:    --- Support

// region:    --- PaneWidget

impl PaneWidget for TaskWidget<'_> {
    fn measure(&self, width: usize) -> usize {
        self.call_rows(width).len().max(1)
    }

    fn paint(&self, area: Rect, buf: &mut Buffer, ctx: PaintCtx<'_>) {
        let lane = self.lane();
        let width = usize::from(area.width);
        let base = lane_text_style(lane.kind, ctx.theme);
        let marker = lane_marker_style(lane.kind, ctx.theme);
        let call = self.call_line();
        let indicator = call
            .indicator(ctx.now_millis)
            .map(|glyph| format!("{glyph} "));
        let status = status_style(self.entry.status, ctx.theme);

        let mut y = area.y;
        for (row_index, spans) in self.call_rows(width).into_iter().enumerate() {
            if y >= area.bottom() {
                return;
            }
            let continuation = row_index > 0;
            let gutter = lane.prefix(continuation);
            let leading = if continuation {
                None
            } else {
                indicator.as_deref().map(|text| (text, status))
            };
            paint_row(
                buf,
                y,
                width,
                &RowPaint {
                    gutter: Some((&gutter, marker)),
                    leading,
                    spans: &spans,
                    base,
                    bg: None,
                },
                ctx.theme,
            );
            y = y.saturating_add(1);
        }
    }
}

// endregion:    --- PaneWidget
