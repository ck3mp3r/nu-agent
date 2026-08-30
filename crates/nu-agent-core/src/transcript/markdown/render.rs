use crate::transcript::ir::{ContentLine, StyleHint};

use super::{
    projector::project_markdown_to_lines_inner, sanitize::sanitize_assistant_visible_markdown,
};

fn fallback_plain_text_lines(markdown: &str) -> Vec<ContentLine> {
    markdown
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .split('\n')
        .filter(|line| !line.trim().is_empty())
        .map(|line| ContentLine::single(line.to_string(), StyleHint::Normal))
        .collect::<Vec<_>>()
}

pub fn project_markdown_to_lines(markdown: &str, max_width: Option<u16>) -> Vec<ContentLine> {
    let sanitized = sanitize_assistant_visible_markdown(markdown);
    let projected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        project_markdown_to_lines_inner(&sanitized, max_width)
    }));
    match projected {
        Ok(lines) if !lines.is_empty() => lines,
        Ok(lines) if sanitized.trim().is_empty() => lines,
        Ok(_) | Err(_) => fallback_plain_text_lines(&sanitized),
    }
}

pub fn rendered_line_to_plain_text(line: &ContentLine) -> String {
    line.spans
        .iter()
        .map(|span| span.text.as_str())
        .collect::<String>()
}

/// Strip a single surrounding fenced code block from `text` when the entire
/// body is wrapped in one, returning the inner content unchanged otherwise.
///
/// LLM summarizers often echo the compaction template's ```` ``` ```` fences,
/// which causes the whole summary to render as a code block instead of
/// markdown. This unwraps exactly one leading/trailing fence pair so the inner
/// markdown projects normally. Content that is not a single fenced block is
/// returned untouched (leading/trailing whitespace still trimmed).
pub fn unwrap_single_fenced_block(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return trimmed.to_string();
    }
    let Some(first_line_end) = trimmed.find('\n') else {
        // Single line: not a fenced block unless the whole line is just a fence.
        return trimmed.to_string();
    };
    let (first_line, rest) = trimmed.split_at(first_line_end);
    let first_line = first_line.trim_end();
    if !first_line.starts_with("```") {
        return trimmed.to_string();
    }
    let rest = rest.strip_prefix('\n').unwrap_or(rest);
    // Strip the leading fence so its content is not trapped in a code block,
    // whether or not a matching closing fence is present.
    let Some(close) = rest.rfind("\n```") else {
        return rest.trim().to_string();
    };
    let inner = &rest[..close];
    inner.trim().to_string()
}
