use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::agent::ui::tui::rendering::theme::TuiTheme;

use super::code_blocks::{CodeBlockState, fence_language_hint, highlighted_code_lines};

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
    fn current(&self) -> Style {
        let mut style = Style::default();
        if self.emphasis_depth > 0 {
            style = style.add_modifier(Modifier::ITALIC);
        }
        if self.strong_depth > 0 {
            style = style.add_modifier(Modifier::BOLD);
        }
        style
    }
}

#[derive(Debug, Clone)]
struct ListState {
    ordered_next: Option<u64>,
}

#[derive(Debug, Default)]
struct Projector {
    lines: Vec<Line<'static>>,
    current_spans: Vec<Span<'static>>,
    style_state: StyleState,
    list_stack: Vec<ListState>,
    blockquote_depth: usize,
    heading_depth: usize,
    link_destinations: Vec<String>,
    image_destinations: Vec<String>,
    code_block: Option<CodeBlockState>,
    pending_prefix: bool,
    theme: TuiTheme,
    table: Option<TableBuffer>,
}

impl Projector {
    fn push_unsupported_fallback_text(&mut self, text: &str) {
        self.push_wrapped_text(text, Style::default());
    }

    fn emit_link_suffix(&mut self) {
        if let Some(dest) = self.link_destinations.pop() {
            self.push_text(&format!(" ({dest})"), Style::default());
        }
    }

    fn emit_image_suffix(&mut self) {
        if let Some(dest) = self.image_destinations.pop() {
            self.push_text(&format!(" (image: {dest})"), Style::default());
        }
    }

    fn push_text(&mut self, text: &str, style: Style) {
        if text.is_empty() {
            return;
        }
        self.ensure_prefix();
        self.current_spans
            .push(Span::styled(text.to_string(), style));
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
            self.current_spans.push(Span::raw(prefix));
        }
    }

    fn flush_line(&mut self) {
        if self.current_spans.is_empty() {
            self.pending_prefix = true;
            return;
        }
        let spans = std::mem::take(&mut self.current_spans);
        self.lines.push(Line::from(spans));
        self.pending_prefix = true;
    }

    fn push_wrapped_text(&mut self, text: &str, style: Style) {
        let mut parts = text.split('\n').peekable();
        while let Some(part) = parts.next() {
            if !part.is_empty() {
                self.push_text(part, style);
            }
            if parts.peek().is_some() {
                self.flush_line();
            }
        }
    }

    fn render_code_block(&mut self, block: CodeBlockState) {
        for token_line in highlighted_code_lines(&block, &self.theme) {
            self.push_text("    ", Style::default());
            for (text, style) in token_line {
                self.push_text(&text, style);
            }
            self.flush_line();
        }
    }

    fn render_table(&mut self, table: TableBuffer) {
        let all_rows = std::iter::once(&table.header_row).chain(table.data_rows.iter());
        let col_count = table.header_row.len();

        // Measure column widths
        let mut widths = vec![0usize; col_count];
        for row in all_rows.clone() {
            for (i, cell) in row.iter().enumerate() {
                if i < col_count {
                    widths[i] = widths[i].max(cell.len());
                }
            }
        }

        // Render header row (bold)
        let header_style = Style::default().add_modifier(Modifier::BOLD);
        for (i, cell) in table.header_row.iter().enumerate() {
            if i > 0 {
                self.push_text(" │ ", Style::default());
            }
            let padded = format!(
                " {:<width$} ",
                cell,
                width = widths.get(i).copied().unwrap_or(0)
            );
            self.push_text(&padded, header_style);
        }
        self.flush_line();

        // Render separator
        let sep_parts: Vec<String> = widths.iter().map(|w| "─".repeat(w + 2)).collect();
        let sep = sep_parts.join("┼");
        self.push_text(&sep, Style::default());
        self.flush_line();

        // Render data rows
        for row in &table.data_rows {
            for (i, cell) in row.iter().enumerate() {
                if i > 0 {
                    self.push_text(" │ ", Style::default());
                }
                let padded = format!(
                    " {:<width$} ",
                    cell,
                    width = widths.get(i).copied().unwrap_or(0)
                );
                self.push_text(&padded, Style::default());
            }
            self.flush_line();
        }
    }

    fn on_start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => {}
            Tag::Heading { .. } => {
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
                self.flush_line();
                self.blockquote_depth = self.blockquote_depth.saturating_add(1);
            }
            Tag::List(start) => {
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
                    self.push_text(&(indent + &marker), Style::default());
                }
            }
            Tag::CodeBlock(kind) => {
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
                    self.render_table(table);
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

                let mut style = self.style_state.current();
                if self.heading_depth > 0 {
                    style = style.add_modifier(Modifier::BOLD);
                }
                self.push_wrapped_text(&text, style);
            }
            Event::Code(text) => {
                if let Some(t) = self.table.as_mut() {
                    t.current_cell.push_str(&text);
                    return;
                }

                self.push_text(&text, self.theme.inline_code);
            }
            Event::Html(html) => {
                self.push_unsupported_fallback_text(&html);
            }
            Event::InlineHtml(html) => {
                self.push_unsupported_fallback_text(&html);
            }
            Event::FootnoteReference(label) => {
                self.push_text(&format!("[^{}]", label), Style::default());
            }
            Event::InlineMath(math) => {
                self.push_text(&math, self.theme.inline_code);
            }
            Event::DisplayMath(math) => {
                self.flush_line();
                self.push_text(&math, self.theme.inline_code);
                self.flush_line();
            }
            Event::SoftBreak | Event::HardBreak => {
                self.flush_line();
            }
            Event::Rule => {
                self.flush_line();
                self.push_text("────────────────", Style::default());
                self.flush_line();
            }
            _ => {}
        }
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        self.flush_line();
        self.lines
    }
}

pub(super) fn project_markdown_to_lines_inner(markdown: &str) -> Vec<Line<'static>> {
    let options =
        Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS | Options::ENABLE_TABLES;
    let parser = Parser::new_ext(markdown, options);
    let mut projector = Projector {
        pending_prefix: true,
        theme: TuiTheme::default(),
        ..Projector::default()
    };
    for event in parser {
        projector.on_event(event);
    }
    projector.finish()
}
