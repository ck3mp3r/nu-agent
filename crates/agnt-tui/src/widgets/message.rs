//! The message family widget: user and assistant prose on the prose lanes.

use agnt_core::transcript::entry::MessageEntry;
use agnt_core::transcript::ir::{ContentLine, MessageRole};
use agnt_core::transcript::markdown::render_markdown_lines;
use agnt_render::catalog::{LaneKind, LaneLine};
use agnt_render::{Color, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::components::{content_budget, lane_marker_style, lane_text_style};
use super::text::{RowPaint, paint_row, wrap_line};
use super::{PaintCtx, PaneWidget};

// region:    --- Types

/// User or assistant prose. The user turn paints its surface across the whole
/// row and keeps the rail on every row; the assistant turn paints plain text
/// between its own blank padding rows.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MessageWidget<'a> {
    pub entry: &'a MessageEntry,
    /// Blank padding rows the planner keeps above the content inside the
    /// widget's area. Zero when the entry is flush or its top padding is
    /// clipped at the frame edge.
    pub content_offset: usize,
}

// endregion: --- Types

// region:    --- Support

impl MessageWidget<'_> {
    fn lane(&self) -> LaneLine {
        LaneLine::new(LaneKind::of_message(self.entry.role))
    }

    /// The projected markdown rows at the content budget. An empty message
    /// still renders one row.
    fn lines(&self, budget: usize) -> Vec<ContentLine> {
        let canvas = u16::try_from(budget).unwrap_or(u16::MAX);
        let mut lines = render_markdown_lines(&self.entry.markdown, Some(canvas));
        if lines.is_empty() {
            lines.push(ContentLine::empty());
        }
        lines
    }

    /// The user turn's surface paint under every row; prose is untinted.
    fn background(&self, theme: &Theme) -> Option<Color> {
        match self.entry.role {
            MessageRole::User => Some(theme.user_surface),
            MessageRole::Assistant => None,
        }
    }
}

// endregion: --- Support

// region:    --- PaneWidget

impl PaneWidget for MessageWidget<'_> {
    fn measure(&self, width: usize) -> usize {
        let lane = self.lane();
        let budget = content_budget(width, lane.prefix_width());
        let rows: usize = self
            .lines(budget)
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
        let bg = self.background(ctx.theme);

        let mut y = area
            .y
            .saturating_add(u16::try_from(self.content_offset).unwrap_or(u16::MAX));
        for (line_index, line) in self.lines(budget).iter().enumerate() {
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
                        bg,
                    },
                    ctx.theme,
                );
                y = y.saturating_add(1);
            }
        }
    }
}

// endregion: --- PaneWidget
