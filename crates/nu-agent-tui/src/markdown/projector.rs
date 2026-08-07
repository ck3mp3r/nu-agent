use nu_agent_core::transcript::ir::{ContentLine, Span as IrSpan, StyleHint};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use unicode_width::UnicodeWidthStr;

use super::code_blocks::{CodeBlockState, fence_language_hint, highlighted_code_lines};

fn latex_to_unicode(latex: &str) -> String {
    const REPLACEMENTS: &[(&str, &str)] = &[
        ("\\rightarrow", "→"),
        ("\\leftarrow", "←"),
        ("\\Rightarrow", "⇒"),
        ("\\Leftarrow", "⇐"),
        ("\\leftrightarrow", "↔"),
        ("\\Leftrightarrow", "⇔"),
        ("\\notin", "∉"),
        ("\\infty", "∞"),
        ("\\mapsto", "↦"),
        ("\\approx", "≈"),
        ("\\equiv", "≡"),
        ("\\subset", "⊂"),
        ("\\supset", "⊃"),
        ("\\emptyset", "∅"),
        ("\\partial", "∂"),
        ("\\nabla", "∇"),
        ("\\forall", "∀"),
        ("\\exists", "∃"),
        ("\\implies", "⟹"),
        ("\\cdot", "·"),
        ("\\sum", "∑"),
        ("\\prod", "∏"),
        ("\\int", "∫"),
        ("\\sqrt", "√"),
        ("\\alpha", "α"),
        ("\\beta", "β"),
        ("\\gamma", "γ"),
        ("\\delta", "δ"),
        ("\\epsilon", "ε"),
        ("\\theta", "θ"),
        ("\\lambda", "λ"),
        ("\\mu", "μ"),
        ("\\pi", "π"),
        ("\\sigma", "σ"),
        ("\\phi", "φ"),
        ("\\omega", "ω"),
        ("\\times", "×"),
        ("\\div", "÷"),
        ("\\pm", "±"),
        ("\\mp", "∓"),
        ("\\leq", "≤"),
        ("\\geq", "≥"),
        ("\\neq", "≠"),
        ("\\land", "∧"),
        ("\\lor", "∨"),
        ("\\neg", "¬"),
        ("\\cup", "∪"),
        ("\\cap", "∩"),
        ("\\to", "→"),
        ("\\in", "∈"),
    ];
    let mut result = latex.to_string();
    for (cmd, unicode) in REPLACEMENTS {
        result = result.replace(cmd, unicode);
    }
    result
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
    link_destinations: Vec<String>,
    image_destinations: Vec<String>,
    code_block: Option<CodeBlockState>,
    pending_prefix: bool,
    table: Option<TableBuffer>,
    /// Maximum canvas width threaded from the render context.
    /// Passed to `render_table`; clamping logic is a separate task.
    max_width: Option<u16>,
    /// True once the projector has emitted any non-empty line.
    has_content: bool,
}

impl Projector {
    fn push_unsupported_fallback_text(&mut self, text: &str) {
        self.push_wrapped_text(text, StyleHint::Normal);
    }

    fn emit_link_suffix(&mut self) {
        if let Some(dest) = self.link_destinations.pop() {
            self.push_text(&format!(" ({dest})"), StyleHint::Normal);
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
            self.current_spans.push(IrSpan::normal(prefix));
        }
    }

    fn flush_line(&mut self) {
        if self.current_spans.is_empty() {
            self.pending_prefix = true;
            return;
        }
        let spans = std::mem::take(&mut self.current_spans);
        self.lines.push(ContentLine::from_spans(spans));
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
        for token_line in highlighted_code_lines(&block) {
            self.push_text("    ", StyleHint::Normal);
            for (text, hint) in token_line {
                self.push_text(&text, hint);
            }
            self.flush_line();
        }
    }

    fn render_table(&mut self, mut table: TableBuffer, max_width: Option<u16>) {
        let col_count = table.header_row.len();

        // Measure column widths using display cell width (unicode_width).
        let mut widths = vec![0usize; col_count];
        for row in std::iter::once(&table.header_row).chain(table.data_rows.iter()) {
            for (i, cell) in row.iter().enumerate() {
                if i < col_count {
                    widths[i] = widths[i].max(UnicodeWidthStr::width(cell.as_str()));
                }
            }
        }

        // Clamp number of columns so total width fits within max_width.
        // Table width = 1 (left │) + sum(col_width + 2) + (col_count - 1) (inter-col │) + 1 (right │)
        //             = 1 + 3*col_count + sum(widths)
        // Always keep at least 1 column.
        if let Some(max) = max_width {
            while widths.len() > 1 {
                let total = 1 + 3 * widths.len() + widths.iter().sum::<usize>();
                if total <= max as usize {
                    break;
                }
                widths.pop();
                table.header_row.pop();
                for row in &mut table.data_rows {
                    row.pop();
                }
            }
        }

        let active_cols = widths.len();

        // Top border: ╭──...──┬──...──╮
        let top = format!(
            "╭{}╮",
            widths
                .iter()
                .map(|w| "─".repeat(w + 2))
                .collect::<Vec<_>>()
                .join("┬")
        );
        self.push_text(&top, StyleHint::Normal);
        self.flush_line();

        // Header row: │ cell │ cell │
        let header_hint = StyleHint::MdBold;
        self.push_text("│", StyleHint::Normal);
        for (i, cell) in table.header_row.iter().enumerate() {
            let cell_width = UnicodeWidthStr::width(cell.as_str());
            let pad_count = widths[i].saturating_sub(cell_width);
            let padded = format!(" {}{} ", cell, " ".repeat(pad_count));
            self.push_text(&padded, header_hint.clone());
            if i + 1 < active_cols {
                self.push_text("│", StyleHint::Normal);
            }
        }
        self.push_text("│", StyleHint::Normal);
        self.flush_line();

        // Separator: ├──...──┼──...──┤
        let sep = format!(
            "├{}┤",
            widths
                .iter()
                .map(|w| "─".repeat(w + 2))
                .collect::<Vec<_>>()
                .join("┼")
        );
        self.push_text(&sep, StyleHint::Normal);
        self.flush_line();

        // Data rows: │ cell │ cell │
        for row in &table.data_rows {
            self.push_text("│", StyleHint::Normal);
            for (i, cell) in row.iter().enumerate().take(active_cols) {
                let col_width = widths.get(i).copied().unwrap_or(0);
                let cell_width = UnicodeWidthStr::width(cell.as_str());
                let pad_count = col_width.saturating_sub(cell_width);
                let padded = format!(" {}{} ", cell, " ".repeat(pad_count));
                self.push_text(&padded, StyleHint::Normal);
                if i + 1 < active_cols {
                    self.push_text("│", StyleHint::Normal);
                }
            }
            self.push_text("│", StyleHint::Normal);
            self.flush_line();
        }

        // Bottom border: ╰──...──┴──...──╯
        let bottom = format!(
            "╰{}╯",
            widths
                .iter()
                .map(|w| "─".repeat(w + 2))
                .collect::<Vec<_>>()
                .join("┴")
        );
        self.push_text(&bottom, StyleHint::Normal);
        self.flush_line();
    }

    fn on_start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => {
                self.insert_block_separator();
            }
            Tag::Heading { .. } => {
                self.insert_block_separator();
                self.flush_line();
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
                    self.push_text(&(indent + &marker), StyleHint::Normal);
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
            }
            TagEnd::CodeBlock => {
                self.flush_line();
                if let Some(block) = self.code_block.take() {
                    self.render_code_block(block);
                }
            }
            TagEnd::Link => {
                self.emit_link_suffix();
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
                    StyleHint::MdBold
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
