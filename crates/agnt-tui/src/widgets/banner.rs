//! The banner family widget: centered banner text on the banner lane.

use agnt_core::transcript::entry::BannerEntry;
use agnt_core::transcript::ir::{ContentLine, StyleHint};
use agnt_render::catalog::{LaneKind, LaneLine};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::components::{content_budget, lane_marker_style, lane_text_style};
use super::text::{RowPaint, paint_row, wrap_line};
use super::{PaintCtx, PaneWidget};

// region:    --- Types

/// Centered banner text: every row shifts right so the whole line (gutter plus
/// text) centers in the pane.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BannerWidget<'a> {
    pub entry: &'a BannerEntry,
}

// endregion: --- Types

// region:    --- PaneWidget

impl BannerWidget<'_> {
    fn lane(&self) -> LaneLine {
        LaneLine::new(LaneKind::Banner)
    }

    /// One content line per source line; an empty banner still renders one row.
    fn lines(&self) -> Vec<ContentLine> {
        let banner = &self.entry.text;
        if banner.is_empty() {
            return vec![ContentLine::empty()];
        }
        banner
            .lines()
            .map(|line| ContentLine::single(line.to_string(), StyleHint::Normal))
            .collect()
    }
}

impl PaneWidget for BannerWidget<'_> {
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
                let gutter = centered_gutter(&lane, continuation, &spans, width);
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

// region:    --- Support

/// The lane gutter shifted right so the whole row centers: the padding plus
/// the lane prefix. The row's width is the gutter and the content together.
fn centered_gutter(
    lane: &LaneLine,
    continuation: bool,
    spans: &[agnt_core::transcript::ir::Span],
    width: usize,
) -> String {
    let prefix = lane.prefix(continuation);
    let content: usize = spans.iter().map(|span| span.text.chars().count()).sum();
    let padding = width.saturating_sub(prefix.chars().count() + content) / 2;
    format!("{}{}", " ".repeat(padding), prefix)
}

// endregion: --- Support
