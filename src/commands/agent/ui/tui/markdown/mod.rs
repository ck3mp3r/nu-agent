mod code_blocks;
mod projector;
mod sanitize;

#[cfg(test)]
mod test;

use ratatui::text::{Line, Span};

use self::{projector::project_markdown_to_lines_inner, sanitize::sanitize_assistant_visible_markdown};

fn fallback_plain_text_lines(markdown: &str) -> Vec<Line<'static>> {
    markdown
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .split('\n')
        .filter(|line| !line.trim().is_empty())
        .map(|line| Line::from(vec![Span::raw(line.to_string())]))
        .collect::<Vec<_>>()
}

pub fn project_markdown_to_lines(markdown: &str) -> Vec<Line<'static>> {
    let sanitized = sanitize_assistant_visible_markdown(markdown);
    let projected = std::panic::catch_unwind(|| project_markdown_to_lines_inner(&sanitized));
    match projected {
        Ok(lines) if !lines.is_empty() => lines,
        Ok(lines) if sanitized.trim().is_empty() => lines,
        Ok(_) | Err(_) => fallback_plain_text_lines(&sanitized),
    }
}

pub fn rendered_line_to_plain_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>()
}
