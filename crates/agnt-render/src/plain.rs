//! Plain-text backend: flat text, no lanes, no backgrounds, no backend-added
//! margins.
//!
//! Two uses: plain logging output, and the baseline for cross-backend golden
//! tests — its output is the content every styled backend must carry.

use agnt_core::transcript::entry::TranscriptEntry;
use agnt_core::transcript::ir::{ContentLine, Span, StyleHint};
use agnt_core::transcript::markdown::{render_markdown_lines, rendered_line_to_plain_text};

use crate::backend::{RenderBackend, RowRect};
use crate::catalog::{CatalogComponent, SurfaceRow};
use crate::theme::Theme;

// region:    --- Types

/// The plain backend's output: one flat text row per visual row.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlainBackend {
    lines: Vec<String>,
}

impl PlainBackend {
    /// Rows written so far.
    pub fn output(&self) -> &[String] {
        &self.lines
    }

    /// Number of rows written so far.
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// The output as one newline-joined string.
    pub fn to_text(&self) -> String {
        self.lines.join("\n")
    }

    /// Drop every row, so the backend can paint a new frame.
    pub fn clear(&mut self) {
        self.lines.clear();
    }

    /// Render one catalog component as flat text rows. The plain backend is
    /// the component baseline: content only, no lanes, no backgrounds, no
    /// padding.
    pub fn render_component(&mut self, component: &CatalogComponent) {
        self.lines.extend(component_rows(component));
    }
}

impl RenderBackend for PlainBackend {
    fn render(&mut self, entry: &TranscriptEntry, rect: RowRect, _theme: &Theme) {
        if self.lines.len() < rect.row {
            self.lines.resize(rect.row, String::new());
        }
        self.lines.extend(entry_rows(
            entry,
            rect.width,
            rect.content_offset,
            rect.rows,
        ));
    }

    fn measure(&self, entry: &TranscriptEntry, width: usize) -> usize {
        content_rows(entry, width).len()
    }
}

// endregion: --- Types

// region:    --- Support

/// One catalog component's flat text rows. The code surface keeps its padding
/// rows as blank rows; every other component contributes content only.
fn component_rows(component: &CatalogComponent) -> Vec<String> {
    match component {
        CatalogComponent::LaneLine(line) => vec![line.prefix(false)],
        CatalogComponent::CallLineRow(row) => vec![row.plain_text(0)],
        CatalogComponent::StatRow(row) => row.text().map_or_else(Vec::new, |text| vec![text]),
        CatalogComponent::DiffView(view) => {
            view.lines.iter().map(rendered_line_to_plain_text).collect()
        }
        CatalogComponent::CodeSurface(surface) => {
            let mut rows = vec![String::new()];
            for row in &surface.rows {
                match row {
                    SurfaceRow::Text(line) => rows.push(rendered_line_to_plain_text(line)),
                    SurfaceRow::Diff(view) => {
                        rows.extend(view.lines.iter().map(rendered_line_to_plain_text))
                    }
                }
            }
            rows.push(String::new());
            rows
        }
    }
}

/// The entry's flat text rows for its rect at `width`: `content_offset` blank
/// padding rows, the content lines flattened to their span text and wrapped,
/// then blanks up to `rows`. The backend adds no margin of its own; the blank
/// rows are the entry's own padding, carried by the rect.
fn entry_rows(
    entry: &TranscriptEntry,
    width: usize,
    content_offset: usize,
    rows: usize,
) -> Vec<String> {
    let content = content_rows(entry, width);
    let lead = content_offset.min(rows);
    let mut out = vec![String::new(); lead];
    out.extend(content.into_iter().take(rows.saturating_sub(lead)));
    out.resize(rows, String::new());
    out
}

/// The entry's content rows at `width`: every content line flattened to its
/// span text, then wrapped. No lane prefix, no background, no padding.
fn content_rows(entry: &TranscriptEntry, width: usize) -> Vec<String> {
    let width = width.max(1);
    content_lines(entry, width)
        .iter()
        .flat_map(|line| wrap_line(&rendered_line_to_plain_text(line), width))
        .collect()
}

/// The entry's content lines. Each family projects from its own data; the
/// tool entry renders its call line and its unified display, `result.or(preview)`,
/// no more.
fn content_lines(entry: &TranscriptEntry, width: usize) -> Vec<ContentLine> {
    match entry {
        TranscriptEntry::Message(message) => {
            let canvas_width = u16::try_from(width).unwrap_or(u16::MAX);
            render_markdown_lines(&message.markdown, Some(canvas_width))
        }
        TranscriptEntry::Tool(tool) => {
            let name = tool.call.name.0.as_str();
            let summary = tool.call.call_line.summary.as_str();
            let mut spans = Vec::new();
            if name.is_empty() {
                spans.push(Span::new(summary.to_string(), StyleHint::Muted));
            } else {
                spans.push(Span::new(name.to_string(), StyleHint::Emphasis));
                if !summary.is_empty() {
                    spans.push(Span::new(format!(" {summary}"), StyleHint::Muted));
                }
            }
            let mut lines = vec![ContentLine::from_spans(spans)];
            if let Some(display) = tool.result.as_ref().or(tool.preview.as_ref()) {
                lines.extend(display.project_lines());
            }
            lines
        }
        TranscriptEntry::Notice(notice) => text_lines(&notice.text),
        TranscriptEntry::Banner(banner) => text_lines(&banner.text),
    }
}

/// One content line per source line; an empty text still yields one row.
fn text_lines(text: &str) -> Vec<ContentLine> {
    if text.is_empty() {
        return vec![ContentLine::empty()];
    }
    text.lines()
        .map(|line| ContentLine::single(line.to_string(), StyleHint::Normal))
        .collect()
}

/// Greedy first-fit word wrap at `width` display columns. Returns at least one
/// row, so an empty input still renders as one empty row. The options mirror
/// the TUI's prose wrap so both backends break lines at the same point.
fn wrap_line(text: &str, width: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    textwrap::wrap(
        text,
        textwrap::Options::new(width).word_splitter(textwrap::WordSplitter::NoHyphenation),
    )
    .into_iter()
    .map(|row| row.into_owned())
    .collect()
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "../test/plain.rs"]
mod plain_test;

// endregion: --- Tests
