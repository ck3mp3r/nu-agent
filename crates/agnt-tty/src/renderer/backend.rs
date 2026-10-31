//! The ANSI backend: the render layer's terminal backend over the entry model.
//!
//! The TTY's own character is the row decoration: a family prefix (`[user] `,
//! `[tool] `, `[compaction] `, `[system] `) and the tool status indicator, with
//! ANSI styling gated by `use_color`. The content itself is the data level's
//! projection — the same functions the plain backend consumes — so both
//! backends carry the same rows and differ only in decoration. The
//! cross-backend golden test pins that agreement.

use agnt_core::transcript::entry::TranscriptEntry;
use agnt_core::transcript::ir::{ContentLine, MessageRole, NoticeKind, Span, StyleHint};
use agnt_core::transcript::markdown::{render_markdown_lines, rendered_line_to_plain_text};
use agnt_render::{RenderBackend, RowRect, Theme};

use crate::ansi::style_text;

// region:    --- Types

/// The ANSI backend's output: one decorated row per visual row.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TtyBackend {
    lines: Vec<String>,
    use_color: bool,
    now_millis: u128,
}

// endregion: --- Types

// region:    --- Constructors

impl TtyBackend {
    /// A backend that emits ANSI escapes only when `use_color` is true.
    pub fn new(use_color: bool) -> Self {
        Self {
            lines: Vec::new(),
            use_color,
            now_millis: 0,
        }
    }

    /// Set the clock reading the status indicator animates at.
    pub fn with_now_millis(mut self, now_millis: u128) -> Self {
        self.now_millis = now_millis;
        self
    }
}

// endregion: --- Constructors

// region:    --- Accessors

impl TtyBackend {
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
}

// endregion: --- Accessors

// region:    --- RenderBackend

impl RenderBackend for TtyBackend {
    fn render(&mut self, entry: &TranscriptEntry, rect: RowRect, _theme: &Theme) {
        if self.lines.len() < rect.row {
            self.lines.resize(rect.row, String::new());
        }
        let content = self.entry_rows(entry, rect.width);
        let lead = rect.content_offset.min(rect.rows);
        let mut rows = vec![String::new(); lead];
        rows.extend(content.into_iter().take(rect.rows.saturating_sub(lead)));
        rows.resize(rect.rows, String::new());
        self.lines.extend(rows);
    }

    fn measure(&self, entry: &TranscriptEntry, width: usize) -> usize {
        self.entry_rows(entry, width).len()
    }
}

// endregion: --- RenderBackend

// region:    --- Support

impl TtyBackend {
    /// One entry's decorated content rows at `width`. Only the entry's first
    /// content row carries the family prefix and, for a tool, the status
    /// indicator; a wrapped continuation row carries its content alone. The
    /// entry's padding rows are the rect's: `render` emits them as blank rows
    /// above and below these, so both backends carry the same blanks.
    fn entry_rows(&self, entry: &TranscriptEntry, width: usize) -> Vec<String> {
        let width = width.max(1);
        let head = self.head(entry);
        let mut rows = Vec::new();
        for (line_index, line) in content_lines(entry, width).iter().enumerate() {
            for (row_index, row) in styled_rows(line, width, self.use_color)
                .into_iter()
                .enumerate()
            {
                if line_index == 0 && row_index == 0 {
                    rows.push(format!("{head}{row}"));
                } else {
                    rows.push(row);
                }
            }
        }
        rows
    }

    /// The row-0 decoration for one entry: the family prefix plus, for a tool,
    /// the status indicator and the space that separates it from the call
    /// line. Families with no prefix return an empty string.
    fn head(&self, entry: &TranscriptEntry) -> String {
        match entry {
            TranscriptEntry::Message(message) => match message.role {
                MessageRole::User => "[user] ".to_string(),
                MessageRole::Assistant => String::new(),
            },
            TranscriptEntry::Tool(tool) => {
                let indicator = tool.status.indicator_char(self.now_millis);
                format!("[tool] {indicator} ")
            }
            TranscriptEntry::Notice(notice) => match notice.kind {
                NoticeKind::Compaction => "[compaction] ".to_string(),
                NoticeKind::System => "[system] ".to_string(),
            },
            TranscriptEntry::Banner(_) => String::new(),
        }
    }
}

/// The entry's content lines at `width`. Each family projects from its own
/// data; the tool entry renders its call line and its unified display,
/// `result.or(preview)`, no more. This mirrors the plain backend's projection
/// line for line: the backends share the data, not a helper layer.
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

/// One content line's decorated rows at `width`: the span text wrapped with the
/// plain backend's options, then every row's span slice styled per hint. The
/// wrapper's rows are contiguous substrings of the span text, so a forward scan
/// locates each row's byte range and the spans are sliced at it.
fn styled_rows(line: &ContentLine, width: usize, use_color: bool) -> Vec<String> {
    let text = rendered_line_to_plain_text(line);
    let rows = wrap_line(&text, width);
    if line.spans.len() <= 1 {
        let hint = line
            .spans
            .first()
            .map_or(StyleHint::Normal, |span| span.hint.clone());
        return rows
            .into_iter()
            .map(|row| style_text(&row, &hint, use_color))
            .collect();
    }

    let mut cursor = 0usize;
    rows.into_iter()
        .map(|row| {
            let Some(start) = text[cursor..].find(row.as_str()).map(|rel| cursor + rel) else {
                return row;
            };
            let end = start + row.len();
            cursor = end;
            styled_slice(&line.spans, start, end, use_color)
        })
        .collect()
}

/// The styled text covering byte range `[start, end)` of the joined span text.
fn styled_slice(spans: &[Span], start: usize, end: usize, use_color: bool) -> String {
    let mut styled = String::new();
    let mut offset = 0usize;
    for span in spans {
        let span_start = offset;
        let span_end = offset + span.text.len();
        offset = span_end;
        let from = start.max(span_start);
        let to = end.min(span_end);
        if from >= to {
            continue;
        }
        styled.push_str(&style_text(
            &span.text[from - span_start..to - span_start],
            &span.hint,
            use_color,
        ));
    }
    styled
}

/// Greedy first-fit word wrap at `width` display columns. Returns at least one
/// row, so an empty input still renders as one empty row. The options mirror
/// the plain backend's wrap so both backends break lines at the same point.
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
#[path = "../../test/renderer/backend.rs"]
mod backend_test;

#[cfg(test)]
#[path = "../../test/renderer/golden.rs"]
mod golden_test;

#[cfg(test)]
#[path = "../../test/renderer/parity.rs"]
mod parity_test;

// endregion: --- Tests
