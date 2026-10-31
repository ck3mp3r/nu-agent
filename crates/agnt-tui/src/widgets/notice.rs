//! The notice family widget: compaction and system notices on their lanes.

use agnt_core::transcript::entry::NoticeEntry;
use agnt_core::transcript::ir::{ContentLine, StyleHint};
use agnt_render::catalog::{LaneKind, LaneLine};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::components::{content_budget, lane_marker_style, lane_text_style};
use super::text::{RowPaint, paint_row, wrap_line};
use super::{PaintCtx, PaneWidget};

// region:    --- Types

/// A compaction or system notice: one content line per source line on the
/// notice lane, no surface.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NoticeWidget<'a> {
    pub entry: &'a NoticeEntry,
}

// endregion: --- Types

// region:    --- PaneWidget

impl NoticeWidget<'_> {
    fn lane(&self) -> LaneLine {
        LaneLine::new(LaneKind::of_notice(self.entry.kind))
    }

    /// One content line per source line; an empty notice still renders one row.
    fn lines(&self) -> Vec<ContentLine> {
        if self.entry.text.is_empty() {
            return vec![ContentLine::empty()];
        }
        self.entry
            .text
            .lines()
            .map(|line| ContentLine::single(line.to_string(), StyleHint::Normal))
            .collect()
    }
}

impl PaneWidget for NoticeWidget<'_> {
    fn measure(&self, width: usize) -> usize {
        let budget = content_budget(width, self.lane().prefix_width());
        let rows: usize = self
            .lines()
            .iter()
            .map(|line| wrap_line(line, budget).len())
            .sum();
        rows.max(1)
    }

    fn paint(&self, area: Rect, buf: &mut Buffer, ctx: PaintCtx<'_>) {
        let lane = self.lane();
        let width = usize::from(area.width);
        let budget = content_budget(width, lane.prefix_width());
        let base = lane_text_style(lane.kind, ctx.theme);
        let marker = lane_marker_style(lane.kind, ctx.theme);

        let mut y = area.y;
        for (line_index, line) in self.lines().iter().enumerate() {
            for (row_index, spans) in wrap_line(line, budget).into_iter().enumerate() {
                if y >= area.bottom() {
                    return;
                }
                let continuation = line_index > 0 || row_index > 0;
                let gutter = lane.prefix(continuation);
                paint_row(
                    buf,
                    y,
                    width,
                    &RowPaint {
                        gutter: Some((&gutter, marker)),
                        leading: None,
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
}

// endregion: --- PaneWidget
