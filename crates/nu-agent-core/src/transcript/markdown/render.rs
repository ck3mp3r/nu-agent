use crate::transcript::ir::{ContentLine, DiffTint, Span, StyleHint};

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
/// StyleHints (MdCodeKeyword, MdCodePlain, etc.), with no leading indent span
/// (the block sits at the lane column). This bypasses the markdown round-trip
/// so tool-display code keeps its syntax highlighting instead of being flattened
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
        let mut spans = Vec::with_capacity(token_line.len());
        for (text, hint) in token_line {
            spans.push(crate::transcript::ir::Span::new(text, hint));
        }
        // No leading indent span: the code block sits at the lane column (4).
        // hang_indent = 4 so wrapped continuation rows indent further (col 8),
        // emphasizing they are continuations of the previous line.
        lines.push(ContentLine {
            spans,
            hang_indent: 4,
            diff_tint: None,
        });
    }
    lines
}

pub fn rendered_line_to_plain_text(line: &ContentLine) -> String {
    line.spans
        .iter()
        .map(|span| span.text.as_str())
        .collect::<String>()
}

/// Project unified-diff source into ContentLines with per-token syntax
/// highlighting on the code body, a muted line-number gutter, and a line-level
/// [`DiffTint`] for the diff background.
///
/// Each diff body line is split into a muted gutter span (the old/new line
/// numbers) and the bare code body, which is routed through the shared syntect
/// path ([`highlighted_code_lines`]) so the body keeps its syntax colours while
/// the renderer paints the diff background from `diff_tint`. Hunk headers, file
/// headers, and the no-newline marker pass through as single spans with no
/// tint; any line that is not diff content passes through verbatim with its
/// [`annotate_diff_hint`] colour.
///
/// The language comes from the `--- a/<file>` / `+++ b/<file>` header extension
/// when the diff carries one — the authoritative source, because the
/// `ContentKind::Diff` language field carries the coarse `"diff"` marker for
/// every edit display. `language_hint` is the fallback when no file header is
/// present.
pub fn project_diff_lines(source: &str, language_hint: &str) -> Vec<ContentLine> {
    let normalized = source.replace("\r\n", "\n").replace('\r', "\n");
    let language = resolve_diff_language(language_hint, &normalized);
    let mut lines = Vec::new();
    let mut old_line: Option<usize> = None;
    let mut new_line: Option<usize> = None;

    for raw in normalized.split('\n') {
        if raw.is_empty() && lines.is_empty() {
            continue;
        }
        if raw.starts_with("@@") {
            if let Some((old_start, new_start)) = parse_hunk_header_start(raw) {
                old_line = Some(old_start);
                new_line = Some(new_start);
            }
            lines.push(ContentLine::single(raw.to_string(), StyleHint::DiffHunk));
            continue;
        }
        // Unified-diff file headers and the no-newline marker are structure,
        // not hunk content — they carry no line numbers and no tint.
        if raw.starts_with("--- ") || raw.starts_with("+++ ") || raw.starts_with("\\ ") {
            lines.push(ContentLine::single(raw.to_string(), StyleHint::Meta));
            continue;
        }

        let mut chars = raw.chars();
        let prefix = chars.next();
        let body = chars.as_str();

        // Context lines show both numbers; removed lines advance the old
        // counter; added lines advance the new counter. Numbers are
        // right-aligned to 4 columns and the pipe sits against the body.
        let mut spans = Vec::new();
        let tint = match (prefix, old_line, new_line) {
            (Some(' '), Some(old), Some(new)) => {
                spans.push(Span::muted(format!(" {old:>4} {new:>4} │")));
                old_line = Some(old.saturating_add(1));
                new_line = Some(new.saturating_add(1));
                DiffTint::Context
            }
            (Some('-'), Some(old), _) => {
                spans.push(Span::muted(format!("-{old:>4}      │")));
                old_line = Some(old.saturating_add(1));
                DiffTint::Remove
            }
            (Some('+'), _, Some(new)) => {
                spans.push(Span::muted(format!("+     {new:>4} │")));
                new_line = Some(new.saturating_add(1));
                DiffTint::Add
            }
            _ => {
                lines.push(ContentLine::single(
                    raw.to_string(),
                    crate::transcript::items::annotate_diff_hint(raw),
                ));
                continue;
            }
        };

        // No language detected: keep the bare body as one plain span. The
        // highlighter would only echo the text back as MdCodePlain, so skip the
        // syntect round-trip entirely.
        let body_spans = match language.as_deref() {
            Some(language) => {
                let block = CodeBlockState {
                    language_hint: Some(language.to_string()),
                    source: body.to_string(),
                };
                let mut token_lines = highlighted_code_lines(&block);
                // The highlighter appends a newline to each source line; the diff
                // body is a single line, so strip it from the last token. A
                // trailing newline would make the word wrapper emit an extra
                // blank visual row per diff line (task 7bd175d2).
                if let Some((last_text, _)) =
                    token_lines.last_mut().and_then(|line| line.last_mut())
                {
                    *last_text = last_text.trim_end_matches('\n').to_string();
                }
                token_lines
                    .into_iter()
                    .flatten()
                    .map(|(text, hint)| Span::new(text, hint))
                    .collect::<Vec<_>>()
            }
            None => vec![Span::new(body.to_string(), StyleHint::Normal)],
        };
        if body_spans.is_empty() {
            spans.push(Span::new(String::new(), StyleHint::Normal));
        } else {
            spans.extend(body_spans);
        }
        lines.push(ContentLine {
            spans,
            hang_indent: 0,
            diff_tint: Some(tint),
        });
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

// region:    --- Support

/// Resolve the syntax language for a diff: the file-header extension when the
/// diff carries one, otherwise the caller's language hint. Returns `None` when
/// neither yields a language, so the body falls back to plain spans.
///
/// The coarse `"diff"` marker that `ContentKind::Diff` carries for every edit
/// display is not a language — syntect would resolve it to its own Diff grammar
/// and flatten the body to `MdCodePlain`. It is treated as no hint.
fn resolve_diff_language(language_hint: &str, source: &str) -> Option<String> {
    if let Some(extension) = diff_header_extension(source) {
        return Some(extension);
    }
    let hint = language_hint.trim();
    if hint.is_empty() || hint.eq_ignore_ascii_case("diff") {
        return None;
    }
    Some(hint.to_string())
}

/// The file extension from the first `--- a/<file>` or `+++ b/<file>` header,
/// lowercased. `None` when the diff has no header or the header path carries
/// no extension (e.g. `/dev/null`).
fn diff_header_extension(source: &str) -> Option<String> {
    for raw in source.split('\n') {
        let Some(path) = raw
            .strip_prefix("+++ ")
            .or_else(|| raw.strip_prefix("--- "))
        else {
            continue;
        };
        let path = path.split_whitespace().next().unwrap_or_default();
        let path = path.trim_start_matches("a/").trim_start_matches("b/");
        if let Some((_, extension)) = path.rsplit_once('.')
            && !extension.is_empty()
        {
            return Some(extension.to_ascii_lowercase());
        }
    }
    None
}

/// Parse the old/new start line numbers from a `@@ -old_start,old_count
/// +new_start,new_count @@` hunk header.
fn parse_hunk_header_start(line: &str) -> Option<(usize, usize)> {
    let mut parts = line.split_whitespace();
    let old = parts.nth(1)?;
    let new = parts.next()?;
    let old_start = old
        .strip_prefix('-')?
        .split(',')
        .next()?
        .parse::<usize>()
        .ok();
    let new_start = new
        .strip_prefix('+')?
        .split(',')
        .next()?
        .parse::<usize>()
        .ok();
    Some((old_start?, new_start?))
}

// endregion: --- Support

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
