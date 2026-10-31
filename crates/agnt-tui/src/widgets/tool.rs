//! The tool family widget: the call line and the display body.
//!
//! One tool entry for a call's whole life. The call line stays untinted; the
//! display body renders per `ContentKind` — a diff body through the diff view,
//! a code body on the code surface, plain sections as plain rows. The body
//! reads the entry's unified display slot, `result.or(preview)`: the gate
//! preview shows while the call is pending, the completion result takes over,
//! and the preview persists when the completion carries no display.

use agnt_core::transcript::entry::ToolEntry;
use agnt_core::transcript::ir::{ContentKind, ContentLine, Display, StyleHint};
use agnt_core::transcript::markdown::{project_code_block_lines, project_diff_lines};
use agnt_render::catalog::{
    CallLineRow, CodeSurface, DiffView, LaneKind, LaneLine, StatRow, SurfaceRow,
};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::components::{
    CodeSurfaceWidget, content_budget, lane_marker_style, lane_text_style, status_style,
};
use super::text::{RowPaint, VisualRow, paint_row, wrap_line};
use super::{PaintCtx, PaneWidget};

// region:    --- Types

/// One tool call: its call line and its display body.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ToolWidget<'a> {
    pub entry: &'a ToolEntry,
}

/// The display body rows. A display carrying code or diff content renders on
/// the code surface; every other display renders as plain rows.
enum DisplayBody {
    Plain(Vec<ContentLine>),
    Surface(CodeSurface),
}

// endregion: --- Types

// region:    --- Support

impl ToolWidget<'_> {
    fn lane(&self) -> LaneLine {
        LaneLine::new(LaneKind::Tool)
    }

    fn call_line(&self) -> CallLineRow {
        CallLineRow::new(
            self.entry.call.name.0.clone(),
            self.entry.call.call_line.summary.clone(),
            Some(self.entry.status),
        )
    }

    /// The display the body renders: the entry's unified display slot.
    ///
    /// The result takes over at completion; the preview shows while the call
    /// is pending and persists when the completion carries no display. One
    /// slot, so the completion body paints once — from the entry itself.
    fn display(&self) -> Option<&Display> {
        self.entry.display()
    }

    fn body(&self) -> DisplayBody {
        let Some(display) = self.display() else {
            return DisplayBody::Plain(Vec::new());
        };
        if display.has_code_or_diff() {
            DisplayBody::Surface(display_surface(display))
        } else {
            DisplayBody::Plain(display_lines(display))
        }
    }

    /// Whether the displayed rows paint as a banded code region.
    ///
    /// Only the gate preview is a code-surface block: the pre-wave pane pushed
    /// it with `Fill::Code`. A completion display renders bare — its diff rows
    /// keep their line tints on the pane, its other rows read as prose. The
    /// entry's state decides, not a second field: `result` set means the body
    /// on screen is the completion.
    fn banded(&self) -> bool {
        self.entry.result.is_none()
    }

    /// The call line's wrap budget: the status indicator takes its columns on
    /// row 0, so every call row wraps short of it.
    fn call_budget(&self, width: usize) -> usize {
        crate::state::code_block::content_wrap_width(width, true, self.lane().prefix_width())
    }

    fn call_rows(&self, width: usize) -> Vec<VisualRow> {
        let content = ContentLine::from_spans(self.call_line().spans());
        wrap_line(&content, self.call_budget(width))
    }
}

/// A display carrying code or diff rows: title and label rows, each section's
/// stats row, and the section body. Diff bodies become diff views, code bodies
/// become text rows — both inside one code surface.
fn display_surface(display: &Display) -> CodeSurface {
    let mut rows = Vec::new();
    if let Some(title) = &display.title {
        rows.push(SurfaceRow::Text(ContentLine::single(
            title.clone(),
            StyleHint::Normal,
        )));
    }
    for section in &display.sections {
        if let Some(label) = &section.label {
            rows.push(SurfaceRow::Text(ContentLine::single(
                format!("{} ({})", label, section.kind.language()),
                StyleHint::Normal,
            )));
        }
        if let Some(text) = section
            .stats
            .clone()
            .and_then(|stats| StatRow::new(stats).text())
        {
            rows.push(SurfaceRow::Text(ContentLine::single(
                text,
                StyleHint::Muted,
            )));
        }
        match &section.kind {
            ContentKind::Diff { language, .. } => rows.push(SurfaceRow::Diff(DiffView::new(
                project_diff_lines(&section.content, language),
            ))),
            ContentKind::Code { language } => rows.extend(
                project_code_block_lines(language, &section.content)
                    .into_iter()
                    .map(SurfaceRow::Text),
            ),
            ContentKind::Plain => rows.push(SurfaceRow::Text(ContentLine::single(
                section.content.clone(),
                StyleHint::Normal,
            ))),
        }
    }
    CodeSurface::new(rows)
}

/// A display with no code or diff content: plain rows only.
fn display_lines(display: &Display) -> Vec<ContentLine> {
    let mut lines = Vec::new();
    if let Some(title) = &display.title {
        lines.push(ContentLine::single(title.clone(), StyleHint::Normal));
    }
    for section in &display.sections {
        if let Some(label) = &section.label {
            lines.push(ContentLine::single(
                format!("{} ({})", label, section.kind.language()),
                StyleHint::Normal,
            ));
        }
        if let Some(text) = section
            .stats
            .clone()
            .and_then(|stats| StatRow::new(stats).text())
        {
            lines.push(ContentLine::single(text, StyleHint::Muted));
        }
        lines.extend(text_lines(&section.content));
    }
    lines
}

/// One content line per source line; empty text yields one empty row.
fn text_lines(text: &str) -> Vec<ContentLine> {
    if text.is_empty() {
        return vec![ContentLine::empty()];
    }
    text.lines()
        .map(|line| ContentLine::single(line.to_string(), StyleHint::Normal))
        .collect()
}

// endregion: --- Support

// region:    --- PaneWidget

impl PaneWidget for ToolWidget<'_> {
    fn measure(&self, width: usize) -> usize {
        let gutter_width = self.lane().prefix_width();
        let body_rows = match self.body() {
            DisplayBody::Plain(lines) => {
                let budget = content_budget(width, gutter_width);
                lines
                    .iter()
                    .map(|line| wrap_line(line, budget).len())
                    .sum::<usize>()
            }
            DisplayBody::Surface(surface) => CodeSurfaceWidget {
                surface: &surface,
                gutter_width,
                band: self.banded(),
            }
            .measure(width),
        };
        self.call_rows(width).len() + body_rows
    }

    fn paint(&self, area: Rect, buf: &mut Buffer, ctx: PaintCtx<'_>) {
        let lane = self.lane();
        let width = usize::from(area.width);
        let gutter_width = lane.prefix_width();
        let base = lane_text_style(lane.kind, ctx.theme);
        let marker = lane_marker_style(lane.kind, ctx.theme);
        let call = self.call_line();
        let indicator = call
            .indicator(ctx.now_millis)
            .map(|glyph| format!("{glyph} "));
        let status = status_style(self.entry.status, ctx.theme);
        let body = self.body();

        // The call line always opens the entry and stays untinted; the display
        // body follows it.
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

        match body {
            DisplayBody::Plain(lines) => {
                let budget = content_budget(width, gutter_width);
                let gutter = lane.prefix(true);
                for line in &lines {
                    for spans in wrap_line(line, budget) {
                        if y >= area.bottom() {
                            return;
                        }
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
            DisplayBody::Surface(surface) => {
                let widget = CodeSurfaceWidget {
                    surface: &surface,
                    gutter_width,
                    band: self.banded(),
                };
                let rows = widget.measure(width);
                let rect = Rect {
                    x: area.x,
                    y,
                    width: area.width,
                    height: u16::try_from(rows).unwrap_or(u16::MAX),
                };
                widget.paint(rect, buf, ctx);
            }
        }
    }
}

// endregion: --- PaneWidget
