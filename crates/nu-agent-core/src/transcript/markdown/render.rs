use crate::transcript::ir::{ContentLine, StyleHint};

use super::{
    code_blocks::{CodeBlockState, highlighted_code_lines},
    projector::project_markdown_to_lines_inner,
    sanitize::sanitize_assistant_visible_markdown,
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
        Ok(_) | Err(_) => {
            log::warn!(
                "markdown projection failed; falling back to plain text. sanitized input: {sanitized:?}"
            );
            fallback_plain_text_lines(&sanitized)
        }
    }
}

/// Project a fenced code block directly into ContentLines carrying code
/// StyleHints (MdCodeKeyword, MdCodePlain, etc.), with the same 4-space indent
/// the markdown projector applies. This bypasses the markdown round-trip so
/// tool-display code keeps its syntax highlighting instead of being flattened
/// to plain text.
pub fn project_code_block_lines(language: &str, source: &str) -> Vec<ContentLine> {
    let block = CodeBlockState {
        language_hint: if language.trim().is_empty() {
            None
        } else {
            Some(language.to_string())
        },
        source: source.to_string(),
    };
    let mut lines = Vec::new();
    for mut token_line in highlighted_code_lines(&block) {
        if let Some((last_text, _)) = token_line.last_mut() {
            *last_text = last_text.trim_end_matches('\n').to_string();
        }
        let mut spans = Vec::with_capacity(token_line.len() + 1);
        spans.push(crate::transcript::ir::Span::new(
            "    ".to_string(),
            StyleHint::Normal,
        ));
        for (text, hint) in token_line {
            spans.push(crate::transcript::ir::Span::new(text, hint));
        }
        lines.push(ContentLine::from_spans(spans));
    }
    lines
}

pub fn rendered_line_to_plain_text(line: &ContentLine) -> String {
    line.spans
        .iter()
        .map(|span| span.text.as_str())
        .collect::<String>()
}

/// Project unified-diff source into ContentLines with diff StyleHints
/// (DiffAdd/DiffRemove/DiffHunk/Meta via `annotate_diff_hint`), one line per
/// source line, with the same 4-space indent convention as
/// [`project_code_block_lines`]. Diffs are deliberately NOT routed through
/// syntect: the diff coloring contract lives in the diff hint vocabulary, and
/// a syntax highlighter would flatten every line to MdCode* hints (task
/// 6424470b).
pub fn project_diff_lines(source: &str) -> Vec<ContentLine> {
    let normalized = source.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines = Vec::new();
    for raw in normalized.split('\n') {
        if raw.is_empty() && lines.is_empty() {
            continue;
        }
        lines.push(ContentLine::single(
            format!("    {raw}"),
            crate::transcript::items::annotate_diff_hint(raw),
        ));
    }
    // Drop a trailing artifact row produced by a final newline.
    if lines.last().is_some_and(|line| {
        line.spans
            .first()
            .is_some_and(|span| span.text.trim().is_empty())
    }) {
        lines.pop();
    }
    lines
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
