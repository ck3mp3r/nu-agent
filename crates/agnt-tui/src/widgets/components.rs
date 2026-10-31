//! The catalog components in the TUI's style vocabulary.
//!
//! Each component reads its props from the backend-neutral catalog contract and
//! paints itself with the backend's own styles. The family widgets compose
//! these components; the catalog fixes structure, the backend owns pixels.

use agnt_core::transcript::ir::Span;
use agnt_core::transcript::renderer::ItemStatus;
use agnt_render::catalog::{CodeSurface, DiffView, LaneKind, SurfaceRow};
use agnt_render::{Color, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::text::{RowPaint, diff_tint_bg, fg, paint_row, wrap_line};
use super::{PaintCtx, PaneWidget};

// region:    --- Lane styles

/// The marker glyph style of a lane. Lanes are dim so the gutter stays chrome:
/// the accent marks the entry's own content, not the rail.
pub(crate) fn lane_marker_style(kind: LaneKind, theme: &Theme) -> Style {
    let style = match kind {
        LaneKind::User => fg(theme.user_accent),
        LaneKind::Tool => fg(theme.accent),
        LaneKind::Compaction | LaneKind::Assistant => fg(theme.muted),
        LaneKind::System | LaneKind::Banner => fg(theme.notice),
    };
    style.add_modifier(Modifier::DIM)
}

/// The base text style of a lane's content.
pub(crate) fn lane_text_style(kind: LaneKind, theme: &Theme) -> Style {
    match kind {
        LaneKind::Compaction => fg(theme.muted),
        LaneKind::System => fg(theme.notice),
        LaneKind::Banner => fg(theme.banner),
        LaneKind::User | LaneKind::Tool | LaneKind::Assistant => fg(theme.text),
    }
}

/// The status indicator style of a tool call: the bright status colors, with
/// the cancelled mark carrying the idle muted style and no crossing.
pub(crate) fn status_style(status: ItemStatus, theme: &Theme) -> Style {
    match status {
        ItemStatus::InProgress => fg(theme.running),
        ItemStatus::Done => fg(theme.success),
        ItemStatus::Failed => fg(theme.error),
        ItemStatus::Cancelled => fg(theme.idle),
        ItemStatus::Queued | ItemStatus::Unknown => fg(theme.idle),
    }
}

/// The content wrap budget for a lane gutter: the pane width minus the gutter
/// and the right margin the transcript keeps clear.
pub(crate) fn content_budget(width: usize, gutter_width: usize) -> usize {
    crate::state::code_block::content_wrap_width(width, false, gutter_width)
}

// endregion: --- Lane styles

// region:    --- Types

/// A code or diff surface: the surface rows, painted either as a banded region
/// (the code-surface background plus one padding row above and below) or bare
/// (the rows alone, so a diff body keeps its line tints on the pane and a code
/// body renders as plain text).
#[derive(Debug, Clone, Copy)]
pub(crate) struct CodeSurfaceWidget<'a> {
    pub surface: &'a CodeSurface,
    /// Columns reserved for the lane gutter. A surface never holds the entry's
    /// first row, so the gutter is blank — but it still belongs to the surface's
    /// background.
    pub gutter_width: usize,
    /// Whether the rows paint as a banded region: the code-surface background
    /// and the padding rows above and below it.
    pub band: bool,
}

/// A diff body: rows carrying their own line-number gutter and line tint.
/// `surface` is the background underneath the tints when the view is nested in
/// a code surface; a standalone view paints the tint alone.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DiffViewWidget<'a> {
    pub view: &'a DiffView,
    pub gutter_width: usize,
    pub surface: Option<Color>,
}

// endregion: --- Types

// region:    --- CodeSurfaceWidget

impl CodeSurfaceWidget<'_> {
    /// Painted rows of one surface row: its wrap height.
    fn row_count(row: &SurfaceRow, budget: usize) -> usize {
        match row {
            SurfaceRow::Text(line) => wrap_line(line, budget).len(),
            SurfaceRow::Diff(view) => diff_rows(view, budget),
        }
    }
}

impl PaneWidget for CodeSurfaceWidget<'_> {
    fn measure(&self, width: usize) -> usize {
        if self.surface.is_empty() {
            return 0;
        }
        let budget = content_budget(width, self.gutter_width);
        let rows: usize = self
            .surface
            .rows
            .iter()
            .map(|row| Self::row_count(row, budget))
            .sum();
        rows + if self.band {
            CodeSurface::MARGIN_ROWS
        } else {
            0
        }
    }

    fn paint(&self, area: Rect, buf: &mut Buffer, ctx: PaintCtx<'_>) {
        if self.surface.is_empty() {
            return;
        }
        let width = usize::from(area.width);
        let budget = content_budget(width, self.gutter_width);
        let gutter = " ".repeat(self.gutter_width);
        let surface = self.band.then_some(ctx.theme.code_surface);
        let mut y = area.y;

        if self.band {
            paint_surface_row(buf, y, area, &gutter, &[], ctx.theme, surface);
            y = y.saturating_add(1);
        }

        for row in &self.surface.rows {
            match row {
                SurfaceRow::Text(line) => {
                    for spans in wrap_line(line, budget) {
                        paint_surface_row(buf, y, area, &gutter, &spans, ctx.theme, surface);
                        y = y.saturating_add(1);
                    }
                }
                SurfaceRow::Diff(view) => {
                    let widget = DiffViewWidget {
                        view,
                        gutter_width: self.gutter_width,
                        surface,
                    };
                    let rows = widget.measure(width);
                    let rect = Rect {
                        x: area.x,
                        y,
                        width: area.width,
                        height: u16::try_from(rows).unwrap_or(u16::MAX),
                    };
                    widget.paint(rect, buf, ctx);
                    y = y.saturating_add(u16::try_from(rows).unwrap_or(u16::MAX));
                }
            }
        }

        if self.band {
            paint_surface_row(buf, y, area, &gutter, &[], ctx.theme, surface);
        }
    }
}

// endregion: --- CodeSurfaceWidget

// region:    --- DiffViewWidget

impl PaneWidget for DiffViewWidget<'_> {
    fn measure(&self, width: usize) -> usize {
        let budget = content_budget(width, self.gutter_width);
        diff_rows(self.view, budget)
    }

    fn paint(&self, area: Rect, buf: &mut Buffer, ctx: PaintCtx<'_>) {
        let width = usize::from(area.width);
        let budget = content_budget(width, self.gutter_width);
        let gutter = " ".repeat(self.gutter_width);
        let base = lane_text_style(LaneKind::Tool, ctx.theme);
        let mut y = area.y;

        for line in &self.view.lines {
            let tint = diff_tint_bg(line.diff_tint.as_ref(), ctx.theme);
            let bg = tint.or(self.surface);
            for spans in wrap_line(line, budget) {
                paint_row(
                    buf,
                    y,
                    width,
                    &RowPaint {
                        gutter: Some((&gutter, Style::default())),
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

// endregion: --- DiffViewWidget

// region:    --- Support

/// The visual rows a diff body occupies at `budget` columns.
fn diff_rows(view: &DiffView, budget: usize) -> usize {
    view.lines
        .iter()
        .map(|line| wrap_line(line, budget).len())
        .sum()
}

/// Paint one row of a code surface: the gutter, the spans, and the row
/// background — the code surface for a banded region, nothing for a bare one,
/// with an optional line tint overriding both.
pub(crate) fn paint_surface_row(
    buf: &mut Buffer,
    y: u16,
    area: Rect,
    gutter: &str,
    spans: &[Span],
    theme: &Theme,
    surface: Option<Color>,
) {
    paint_row(
        buf,
        y,
        usize::from(area.width),
        &RowPaint {
            gutter: Some((gutter, Style::default())),
            leading: None,
            spans,
            base: lane_text_style(LaneKind::Tool, theme),
            bg: surface,
        },
        theme,
    );
}

// endregion: --- Support
