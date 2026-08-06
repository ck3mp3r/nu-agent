use nu_agent_core::transcript::ir::{ContentLine, Span as IrSpan, StyleHint};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;

use crate::markdown::project_markdown_to_lines;
use crate::rendering::theme::TuiTheme;

/// Project raw markdown text into IR ContentLines using the existing
/// pulldown-cmark pipeline. Preserves single blank lines between blocks
/// for readability, but collapses consecutive blanks and drops leading/trailing ones.
///
/// `max_width` is threaded through to the projection layer for future width-aware
/// table rendering; it is not yet used for clamping (that is a separate task).
pub fn render_markdown_lines(
    text: &str,
    max_width: Option<u16>,
    theme: &TuiTheme,
) -> Vec<ContentLine> {
    let projected: Vec<ContentLine> = project_markdown_to_lines(text, max_width, theme)
        .into_iter()
        .map(|line| line_to_content_line(&line, theme))
        .collect();

    // Collapse consecutive blank lines to at most one, strip leading/trailing blanks
    let mut result = Vec::with_capacity(projected.len());
    let mut prev_blank = false;
    for line in projected {
        let is_blank = line.spans.is_empty();
        if is_blank {
            if !prev_blank && !result.is_empty() {
                result.push(line);
            }
            prev_blank = true;
        } else {
            result.push(line);
            prev_blank = false;
        }
    }
    // Trim trailing blank
    if result.last().is_some_and(|l| l.spans.is_empty()) {
        result.pop();
    }
    result
}

fn line_to_content_line(line: &Line<'static>, theme: &TuiTheme) -> ContentLine {
    ContentLine {
        spans: line
            .spans
            .iter()
            .map(|span| IrSpan {
                text: span.content.to_string(),
                hint: style_to_hint(&span.style, theme),
            })
            .collect(),
    }
}

fn style_to_hint(style: &Style, theme: &TuiTheme) -> StyleHint {
    if *style == theme.syntax_keyword {
        return StyleHint::MdCodeKeyword;
    }
    if *style == theme.syntax_type {
        return StyleHint::MdCodeType;
    }
    if *style == theme.syntax_function {
        return StyleHint::MdCodeFunction;
    }
    if *style == theme.syntax_variable {
        return StyleHint::MdCodeVariable;
    }
    if *style == theme.syntax_constant {
        return StyleHint::MdCodeConstant;
    }
    if *style == theme.syntax_string {
        return StyleHint::MdCodeString;
    }
    if *style == theme.syntax_number {
        return StyleHint::MdCodeNumber;
    }
    if *style == theme.syntax_operator {
        return StyleHint::MdCodeOperator;
    }
    if *style == theme.syntax_punctuation {
        return StyleHint::MdCodePunctuation;
    }
    if *style == theme.syntax_comment {
        return StyleHint::MdCodeComment;
    }
    if *style == theme.inline_code {
        return StyleHint::MdInlineCode;
    }

    let bold = style.add_modifier.contains(Modifier::BOLD);
    let italic = style.add_modifier.contains(Modifier::ITALIC);
    match (bold, italic) {
        (true, true) => StyleHint::MdBoldItalic,
        (true, false) => StyleHint::MdBold,
        (false, true) => StyleHint::MdItalic,
        (false, false) => StyleHint::Normal,
    }
}
