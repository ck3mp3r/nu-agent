//! The message family widget: user and assistant prose on the prose lanes.

use agnt_render::catalog::{LaneKind, LaneLine};
use agnt_render::transcript::entry::MessageEntry;
use agnt_render::transcript::ir::{ContentLine, MessageRole, Span};
use agnt_render::transcript::markdown::render_markdown_lines;
use agnt_render::{Color, Theme};
use chrono::{DateTime, Local};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use unicode_width::UnicodeWidthStr;

use super::components::{content_budget, lane_marker_style, lane_text_style};
use super::text::{RowPaint, fg, paint_row, wrap_line};
use super::{PaintCtx, PaneWidget};

/// How a footer stamp reads: the local wall clock, minute resolution.
const STAMP_FORMAT: &str = "%Y-%m-%d %H:%M";
/// The separator between two footer segments.
const FOOTER_SEPARATOR: &str = " · ";
/// The rows a footer adds below the prose: one blank margin row, then the row
/// itself.
const FOOTER_ROWS: usize = 2;

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

    /// The footer row, right-aligned in `budget` columns, or `None` when the
    /// entry has no renderable footer.
    ///
    /// The widget renders exactly the facts the entry carries: absent segments
    /// are skipped, never filled. The composition is `agent · model · stamp` in
    /// that ruled order, each segment present only when its fact is. Only the
    /// user turn carries a footer — its facts describe the prompt, not the
    /// reply. A composition wider than the budget clips at it: no wrap, no
    /// second row.
    fn footer(&self, budget: usize) -> Option<String> {
        if self.entry.role != MessageRole::User {
            return None;
        }
        let mut segments: Vec<String> = Vec::new();
        if let Some(agent) = self.entry.agent.as_deref() {
            segments.push(agent.to_string());
        }
        if let Some(model) = self.entry.model.as_deref() {
            segments.push(model.to_string());
        }
        if let Some(millis) = self.entry.timestamp_millis
            && let Some(stamp) = stamp_text(millis)
        {
            segments.push(stamp);
        }
        let text = segments.join(FOOTER_SEPARATOR);
        if text.is_empty() {
            return None;
        }
        let text = clip_to_budget(&text, budget);
        let pad = budget.saturating_sub(UnicodeWidthStr::width(text.as_str()));
        Some(format!("{}{text}", " ".repeat(pad)))
    }
}

/// The stamp a footer shows for `millis`: the local wall clock at minute
/// resolution, or `None` when the value is not a representable instant.
fn stamp_text(millis: i64) -> Option<String> {
    let stamp = DateTime::from_timestamp_millis(millis)?;
    Some(stamp.with_timezone(&Local).format(STAMP_FORMAT).to_string())
}

/// `text` cut to at most `budget` display columns, on a character boundary: a
/// footer never wraps and never spills past the content budget.
fn clip_to_budget(text: &str, budget: usize) -> String {
    if UnicodeWidthStr::width(text) <= budget {
        return text.to_string();
    }
    let mut clipped = String::new();
    let mut used = 0usize;
    for ch in text.chars() {
        let width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + width > budget {
            break;
        }
        used += width;
        clipped.push(ch);
    }
    clipped
}

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
            + if self.footer(budget).is_some() {
                // The footer brings its own margin: one blank row above it.
                FOOTER_ROWS
            } else {
                0
            }
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

        // The footer closes the entry on the user lane, one blank margin row
        // below the prose: the margin carries the surface and nothing else, so
        // the footer reads apart from the body.
        if let Some(footer) = self.footer(budget) {
            // The rail column is constant across the message: the margin row
            // carries the same gutter every other row of the lane carries, its
            // body side blank.
            let gutter = lane.prefix(true);
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
                    spans: &[],
                    base,
                    bg,
                },
                ctx.theme,
            );
            y = y.saturating_add(1);
            if y >= area.bottom() {
                return;
            }
            let style = fg(ctx.theme.muted).add_modifier(Modifier::DIM);
            paint_row(
                buf,
                y,
                width,
                &RowPaint {
                    gutter: Some((&gutter, marker)),
                    leading: None,
                    spans: &[Span::normal(footer)],
                    base: style,
                    bg,
                },
                ctx.theme,
            );
        }
    }
}

// endregion: --- PaneWidget
