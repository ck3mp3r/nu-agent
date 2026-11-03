use crate::transcript::ir::{ContentLine, Span as IrSpan, StyleHint};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
use unicodeit;

use super::code_blocks::{CodeBlockState, fence_language_hint, highlighted_code_lines};

/// Convert LaTeX math to Unicode. Delegates to `unicodeit` for symbol/subscript/
/// superscript/blackboard-bold conversion, and additionally unwraps `\text{...}`
/// (which `unicodeit` leaves as raw LaTeX) to its inner content.
fn latex_to_unicode(latex: &str) -> String {
    let mut result = latex.to_string();
    while let Some(start) = result.find("\\text{") {
        let Some(open) = result[start..].find('{') else {
            break;
        };
        let open = start + open;
        let Some(close) = result[open..].find('}') else {
            break;
        };
        let close = open + close;
        let inner = result[open + 1..close].to_string();
        result.replace_range(start..=close, &inner);
    }
    unicodeit::replace(&result)
}

/// Wrap one table cell's text at `width` display columns. Words break at ASCII
/// spaces; a word longer than the column breaks at character boundaries. A cell
/// line never exceeds its allocated width, provided the caller allocates at
/// least the column's widest single-char display width — textwrap never
/// truncates, so a narrower column would let a wide glyph overflow
/// (`render_table` floors every column there). Always returns at least one
/// line, so an empty cell still occupies its physical row.
fn wrap_table_cell(cell: &str, width: usize) -> Vec<String> {
    let options =
        textwrap::Options::new(width.max(1)).word_splitter(textwrap::WordSplitter::NoHyphenation);
    let mut lines: Vec<String> = textwrap::wrap(cell, options)
        .into_iter()
        .map(|line| line.to_string())
        .collect();
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

#[derive(Debug, Default)]
struct TableBuffer {
    header_row: Vec<String>,
    data_rows: Vec<Vec<String>>,
    current_cell: String,
    current_row: Vec<String>,
    is_header: bool,
}

#[derive(Debug, Clone, Default)]
struct StyleState {
    emphasis_depth: usize,
    strong_depth: usize,
}

impl StyleState {
    fn current(&self) -> StyleHint {
        let strong = self.strong_depth > 0;
        let italic = self.emphasis_depth > 0;
        match (strong, italic) {
            (true, true) => StyleHint::MdBoldItalic,
            (true, false) => StyleHint::MdBold,
            (false, true) => StyleHint::MdItalic,
            (false, false) => StyleHint::Normal,
        }
    }
}

#[derive(Debug, Clone)]
struct ListState {
    ordered_next: Option<u64>,
}

#[derive(Debug, Default)]
struct Projector {
    lines: Vec<ContentLine>,
    current_spans: Vec<IrSpan>,
    style_state: StyleState,
    list_stack: Vec<ListState>,
    blockquote_depth: usize,
    heading_depth: usize,
    /// Heading level of the innermost open heading (1-6), or 0 when no heading
    /// is open. Drives the per-level `MdHeading` hint.
    heading_level: u8,
    /// Nesting depth of open links. While > 0, plain text is the link label.
    link_depth: usize,
    link_destinations: Vec<String>,
    image_destinations: Vec<String>,
    code_block: Option<CodeBlockState>,
    pending_prefix: bool,
    table: Option<TableBuffer>,
    /// Maximum content width threaded from the render context — the wrap
    /// budget, so the table clamp target matches the width its lines are
    /// wrapped at. Passed to `render_table`.
    max_width: Option<u16>,
    /// True once the projector has emitted any non-empty line.
    has_content: bool,
    /// Display columns occupied by the leading marker of the list item being
    /// projected, `Some` while inside a `Tag::Item` and attached to every line
    /// flushed for that item so the renderer can hang-indent wrapped rows.
    item_hang_indent: Option<usize>,
}

impl Projector {
    fn push_unsupported_fallback_text(&mut self, text: &str) {
        self.push_wrapped_text(text, StyleHint::Normal);
    }

    fn emit_link_suffix(&mut self) {
        if let Some(dest) = self.link_destinations.pop() {
            self.push_text(&format!(" ({dest})"), StyleHint::MdLinkUrl);
        }
    }

    fn emit_image_suffix(&mut self) {
        if let Some(dest) = self.image_destinations.pop() {
            self.push_text(&format!(" (image: {dest})"), StyleHint::Normal);
        }
    }

    fn push_text(&mut self, text: &str, hint: StyleHint) {
        if text.is_empty() {
            return;
        }
        self.ensure_prefix();
        self.current_spans.push(IrSpan::new(text.to_string(), hint));
    }

    fn ensure_prefix(&mut self) {
        if !self.pending_prefix {
            return;
        }

        self.pending_prefix = false;
        let mut prefix = String::new();
        if self.blockquote_depth > 0 {
            for _ in 0..self.blockquote_depth {
                prefix.push('│');
                prefix.push(' ');
            }
        }
        if !prefix.is_empty() {
            self.current_spans
                .push(IrSpan::new(prefix, StyleHint::MdQuote));
        }
    }

    fn flush_line(&mut self) {
        if self.current_spans.is_empty() {
            self.pending_prefix = true;
            return;
        }
        let spans = std::mem::take(&mut self.current_spans);
        let mut line = ContentLine::from_spans(spans);
        line.hang_indent = self.item_hang_indent.unwrap_or(0);
        self.lines.push(line);
        self.pending_prefix = true;
        self.has_content = true;
    }

    /// Emit a blank separator line between block-level elements.
    /// Only inserts a single blank line, even if called repeatedly.
    fn insert_block_separator(&mut self) {
        if !self.has_content {
            return;
        }
        // Avoid consecutive blank lines: if the last line is already empty, skip.
        if self.lines.last().is_some_and(|l| l.spans.is_empty()) {
            return;
        }
        self.lines.push(ContentLine::empty());
    }

    fn push_wrapped_text(&mut self, text: &str, hint: StyleHint) {
        let mut parts = text.split('\n').peekable();
        while let Some(part) = parts.next() {
            if !part.is_empty() {
                self.push_text(part, hint.clone());
            }
            if parts.peek().is_some() {
                self.flush_line();
            }
        }
    }

    fn render_code_block(&mut self, block: CodeBlockState) {
        for mut token_line in highlighted_code_lines(&block) {
            // Highlighters keep each source line's trailing newline in its
            // final token. A raw '\n' inside a ContentLine makes the word
            // wrapper emit an extra empty row (blank rows in edit displays),
            // so terminate the token text at the newline before projecting.
            if let Some((last_text, _)) = token_line.last_mut() {
                *last_text = last_text.trim_end_matches('\n').to_string();
            }
            self.push_text("    ", StyleHint::Normal);
            for (text, hint) in token_line {
                self.push_text(&text, hint);
            }
            self.flush_line();
        }
    }

    fn render_table(&mut self, table: TableBuffer, max_width: Option<u16>) {
        let col_count = table.header_row.len();
        if col_count == 0 {
            return;
        }

        // Measure column widths using display cell width (unicode_width).
        let mut widths = vec![0usize; col_count];
        for row in std::iter::once(&table.header_row).chain(table.data_rows.iter()) {
            for (i, cell) in row.iter().enumerate() {
                if i < col_count {
                    widths[i] = widths[i].max(UnicodeWidthStr::width(cell.as_str()));
                }
            }
        }

        // Floor every column at the widest single char in its own cells: textwrap
        // breaks words at char boundaries and never truncates, so a column
        // narrower than one of its chars would emit a cell line wider than the
        // slot and break the border alignment. Never below 1, so no column
        // collapses to zero width.
        let floors: Vec<usize> = (0..col_count)
            .map(|i| {
                std::iter::once(&table.header_row)
                    .chain(table.data_rows.iter())
                    .filter_map(|row| row.get(i))
                    .flat_map(|cell| cell.chars())
                    .filter_map(UnicodeWidthChar::width)
                    .max()
                    .unwrap_or(0)
                    .max(1)
            })
            .collect();

        // Shrink the widest columns above their floor until the table fits
        // max_width. The column count never shrinks: cell contents wrap instead,
        // so no value is ever dropped. Table width = 1 (left │) + sum(col_width
        // + 2) + (col_count - 1) (inter-col │) + 1 (right │) = 1 + 3*col_count +
        // sum(widths). On ties the leftmost widest column shrinks first.
        //
        // Residual: when max_width < 1 + 3*col_count + sum(floors), the table
        // stops at that structural minimum and so exceeds max_width by that
        // bounded excess. Removing it would mean dropping columns or truncating
        // graphemes, both rejected to keep the no-drop invariant.
        if let Some(max) = max_width {
            let budget = max as usize;
            for (i, width) in widths.iter_mut().enumerate() {
                *width = (*width).max(floors[i]);
            }
            while 1 + 3 * col_count + widths.iter().sum::<usize>() > budget {
                let widest = widths
                    .iter()
                    .enumerate()
                    .filter(|(i, w)| **w > floors[*i])
                    .fold(None, |best: Option<usize>, (i, w)| match best {
                        Some(best) if widths[best] >= *w => Some(best),
                        _ => Some(i),
                    });
                let Some(widest) = widest else { break };
                widths[widest] -= 1;
            }
        }

        // A single-line horizontal border at the allocated widths.
        let border = |left: char, joiner: char, right: char| {
            let joiner = joiner.to_string();
            let segments: Vec<String> = widths.iter().map(|w| "─".repeat(w + 2)).collect();
            format!("{left}{}{right}", segments.join(&joiner))
        };

        // Top border: ╭──...──┬──...──╮
        let top = border('╭', '┬', '╮');
        self.push_text(&top, StyleHint::Normal);
        self.flush_line();

        // Header row: every physical line of a wrapped header cell keeps the
        // generic heading hint, so a wrapped header reads as one heading block.
        self.push_table_row(&table.header_row, &widths, StyleHint::MdHeading(0));

        // Separator: ├──...──┼──...──┤
        let sep = border('├', '┼', '┤');
        self.push_text(&sep, StyleHint::Normal);
        self.flush_line();

        // Data rows: │ cell │ cell │, one physical line per wrapped cell line.
        for row in &table.data_rows {
            self.push_table_row(row, &widths, StyleHint::Normal);
        }

        // Bottom border: ╰──...──┴──...──╯
        let bottom = border('╰', '┴', '╯');
        self.push_text(&bottom, StyleHint::Normal);
        self.flush_line();
    }

    /// Emit one logical table row at the already-allocated column widths: each
    /// cell wraps into as many physical lines as its text needs and the row
    /// spans the tallest cell's line count. Shorter cells pad with spaces, so
    /// every physical line carries the complete border structure.
    fn push_table_row(&mut self, cells: &[String], widths: &[usize], cell_hint: StyleHint) {
        let wrapped: Vec<Vec<String>> = widths
            .iter()
            .enumerate()
            .map(|(i, width)| {
                let text = cells.get(i).map(String::as_str).unwrap_or("");
                wrap_table_cell(text, *width)
            })
            .collect();
        let row_count = wrapped.iter().map(Vec::len).max().unwrap_or(1).max(1);

        for row_idx in 0..row_count {
            self.push_text("│", StyleHint::Normal);
            for (i, cell_lines) in wrapped.iter().enumerate() {
                let text = cell_lines.get(row_idx).map(String::as_str).unwrap_or("");
                let pad_count = widths[i].saturating_sub(UnicodeWidthStr::width(text));
                let padded = format!(" {text}{} ", " ".repeat(pad_count));
                self.push_text(&padded, cell_hint.clone());
                self.push_text("│", StyleHint::Normal);
            }
            self.flush_line();
        }
    }

    fn on_start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => {
                self.insert_block_separator();
            }
            Tag::Heading { level, .. } => {
                self.insert_block_separator();
                self.flush_line();
                self.heading_level = level as u8;
                self.heading_depth = self.heading_depth.saturating_add(1);
            }
            Tag::Emphasis => {
                self.style_state.emphasis_depth = self.style_state.emphasis_depth.saturating_add(1);
            }
            Tag::Strong => {
                self.style_state.strong_depth = self.style_state.strong_depth.saturating_add(1);
            }
            Tag::BlockQuote(_) => {
                self.insert_block_separator();
                self.flush_line();
                self.blockquote_depth = self.blockquote_depth.saturating_add(1);
            }
            Tag::List(start) => {
                self.insert_block_separator();
                self.flush_line();
                self.list_stack.push(ListState {
                    ordered_next: start,
                });
            }
            Tag::Item => {
                self.flush_line();
                let depth = self.list_stack.len().saturating_sub(1);
                let indent = "  ".repeat(depth);
                if let Some(list_state) = self.list_stack.last_mut() {
                    let marker = if let Some(next) = list_state.ordered_next.as_mut() {
                        let current = *next;
                        *next = next.saturating_add(1);
                        format!("{current}. ")
                    } else {
                        "• ".to_string()
                    };
                    let marker_width = indent.chars().count() + marker.chars().count();
                    self.item_hang_indent = Some(marker_width);
                    self.push_text(&(indent + &marker), StyleHint::MdListMarker);
                }
            }
            Tag::CodeBlock(kind) => {
                self.insert_block_separator();
                self.flush_line();
                self.code_block = Some(CodeBlockState {
                    language_hint: fence_language_hint(kind),
                    source: String::new(),
                });
            }
            Tag::Link { dest_url, .. } => {
                self.link_destinations.push(dest_url.to_string());
                self.link_depth = self.link_depth.saturating_add(1);
            }
            Tag::Image { dest_url, .. } => {
                self.image_destinations.push(dest_url.to_string());
            }
            Tag::Table(_) => {
                self.insert_block_separator();
                self.flush_line();
                self.table = Some(TableBuffer::default());
            }
            Tag::TableHead => {
                if let Some(t) = self.table.as_mut() {
                    t.is_header = true;
                    t.current_row.clear();
                }
            }
            Tag::TableRow => {
                if let Some(t) = self.table.as_mut() {
                    t.current_row.clear();
                }
            }
            Tag::TableCell => {
                if let Some(t) = self.table.as_mut() {
                    t.current_cell.clear();
                }
            }
            _ => {}
        }
    }

    fn on_end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                self.flush_line();
            }
            TagEnd::Heading(_) => {
                self.heading_depth = self.heading_depth.saturating_sub(1);
                self.heading_level = 0;
                self.flush_line();
            }
            TagEnd::Emphasis => {
                self.style_state.emphasis_depth = self.style_state.emphasis_depth.saturating_sub(1);
            }
            TagEnd::Strong => {
                self.style_state.strong_depth = self.style_state.strong_depth.saturating_sub(1);
            }
            TagEnd::BlockQuote(_) => {
                self.flush_line();
                self.blockquote_depth = self.blockquote_depth.saturating_sub(1);
            }
            TagEnd::List(_) => {
                self.flush_line();
                self.list_stack.pop();
            }
            TagEnd::Item => {
                self.flush_line();
                self.item_hang_indent = None;
            }
            TagEnd::CodeBlock => {
                self.flush_line();
                if let Some(block) = self.code_block.take() {
                    self.render_code_block(block);
                }
            }
            TagEnd::Link => {
                self.emit_link_suffix();
                self.link_depth = self.link_depth.saturating_sub(1);
            }
            TagEnd::Image => {
                self.emit_image_suffix();
            }
            TagEnd::Table => {
                if let Some(table) = self.table.take() {
                    self.render_table(table, self.max_width);
                }
            }
            TagEnd::TableHead => {
                if let Some(t) = self.table.as_mut() {
                    t.header_row = std::mem::take(&mut t.current_row);
                    t.is_header = false;
                }
            }
            TagEnd::TableRow => {
                if let Some(t) = self.table.as_mut() {
                    let row = std::mem::take(&mut t.current_row);
                    t.data_rows.push(row);
                }
            }
            TagEnd::TableCell => {
                if let Some(t) = self.table.as_mut() {
                    let cell = std::mem::take(&mut t.current_cell);
                    t.current_row.push(cell);
                }
            }
            _ => {}
        }
    }

    fn on_event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.on_start(tag),
            Event::End(tag) => self.on_end(tag),
            Event::Text(text) => {
                if let Some(block) = self.code_block.as_mut() {
                    block.source.push_str(&text);
                    return;
                }

                if let Some(t) = self.table.as_mut() {
                    t.current_cell.push_str(&text);
                    return;
                }

                let hint = if self.heading_depth > 0 {
                    if self.heading_level == 0 {
                        StyleHint::MdBold
                    } else {
                        StyleHint::MdHeading(self.heading_level)
                    }
                } else if self.link_depth > 0 && self.style_state.current() == StyleHint::Normal {
                    StyleHint::MdLink
                } else if self.blockquote_depth > 0
                    && self.style_state.current() == StyleHint::Normal
                {
                    StyleHint::MdQuote
                } else {
                    self.style_state.current()
                };
                self.push_wrapped_text(&text, hint);
            }
            Event::Code(text) => {
                if let Some(t) = self.table.as_mut() {
                    t.current_cell.push_str(&text);
                    return;
                }

                self.push_text(&text, StyleHint::MdInlineCode);
            }
            Event::Html(html) => {
                self.push_unsupported_fallback_text(&html);
            }
            Event::InlineHtml(html) => {
                self.push_unsupported_fallback_text(&html);
            }
            Event::FootnoteReference(label) => {
                self.push_text(&format!("[^{}]", label), StyleHint::Normal);
            }
            Event::InlineMath(math) => {
                self.push_text(&latex_to_unicode(&math), StyleHint::Normal);
            }
            Event::DisplayMath(math) => {
                self.insert_block_separator();
                self.flush_line();
                self.push_text(&latex_to_unicode(&math), StyleHint::Normal);
                self.flush_line();
            }
            Event::SoftBreak | Event::HardBreak => {
                self.flush_line();
            }
            Event::Rule => {
                self.insert_block_separator();
                self.flush_line();
                self.push_text("────────────────", StyleHint::Normal);
                self.flush_line();
            }
            _ => {}
        }
    }

    fn finish(mut self) -> Vec<ContentLine> {
        self.flush_line();
        self.lines
    }
}

pub(super) fn project_markdown_to_lines_inner(
    markdown: &str,
    max_width: Option<u16>,
) -> Vec<ContentLine> {
    let options = Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_TABLES
        | Options::ENABLE_MATH;
    let parser = Parser::new_ext(markdown, options);
    let mut projector = Projector {
        pending_prefix: true,
        max_width,
        ..Projector::default()
    };
    for event in parser {
        projector.on_event(event);
    }
    projector.finish()
}
