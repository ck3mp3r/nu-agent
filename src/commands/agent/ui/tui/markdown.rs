use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};
use crate::commands::agent::ui::tui::theme::TuiTheme;

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

#[derive(Debug, Clone)]
struct CodeBlockState;

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
        self.current_spans.push(Span::styled(text.to_string(), style));
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

    fn push_code_block_text(&mut self, text: &str) {
        let mut remaining = text;
        while !remaining.is_empty() {
            if let Some((line, rest)) = remaining.split_once('\n') {
                self.push_text(&format!("    {line}"), Style::default());
                self.flush_line();
                remaining = rest;
            } else {
                self.push_text(&format!("    {remaining}"), Style::default());
                remaining = "";
            }
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
                self.list_stack.push(ListState { ordered_next: start });
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
                let language = match kind {
                    CodeBlockKind::Fenced(label) => {
                        let label = label.trim();
                        if label.is_empty() {
                            None
                        } else {
                            Some(label.to_string())
                        }
                    }
                    CodeBlockKind::Indented => None,
                };
                if let Some(lang) = language.as_deref() {
                    self.push_text(&format!("[code:{lang}]"), Style::default());
                    self.flush_line();
                }
                self.code_block = Some(CodeBlockState);
            }
            Tag::Link { dest_url, .. } => {
                self.link_destinations.push(dest_url.to_string());
            }
            Tag::Image { dest_url, .. } => {
                self.image_destinations.push(dest_url.to_string());
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
                self.code_block = None;
            }
            TagEnd::Link => {
                self.emit_link_suffix();
            }
            TagEnd::Image => {
                self.emit_image_suffix();
            }
            _ => {}
        }
    }

    fn on_event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.on_start(tag),
            Event::End(tag) => self.on_end(tag),
            Event::Text(text) => {
                if self.code_block.is_some() {
                    self.push_code_block_text(&text);
                    return;
                }

                let mut style = self.style_state.current();
                if self.heading_depth > 0 {
                    style = style.add_modifier(Modifier::BOLD);
                }
                self.push_wrapped_text(&text, style);
            }
            Event::Code(text) => {
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

fn fallback_plain_text_lines(markdown: &str) -> Vec<Line<'static>> {
    markdown
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .split('\n')
        .filter(|line| !line.trim().is_empty())
        .map(|line| Line::from(vec![Span::raw(line.to_string())]))
        .collect::<Vec<_>>()
}

fn project_markdown_to_lines_inner(markdown: &str) -> Vec<Line<'static>> {
    let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
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

pub fn project_markdown_to_lines(markdown: &str) -> Vec<Line<'static>> {
    let projected = std::panic::catch_unwind(|| project_markdown_to_lines_inner(markdown));
    match projected {
        Ok(lines) if !lines.is_empty() => lines,
        Ok(lines) if markdown.trim().is_empty() => lines,
        Ok(_) | Err(_) => fallback_plain_text_lines(markdown),
    }
}

pub fn rendered_line_to_plain_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>()
}
