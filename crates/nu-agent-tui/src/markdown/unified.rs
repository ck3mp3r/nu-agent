use nu_agent_core::transcript::ir::{ContentLine, Span as IrSpan, StyleHint};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;

use crate::markdown::project_markdown_to_lines;
use crate::rendering::theme::TuiTheme;

/// Project raw markdown text into IR ContentLines using the existing
/// pulldown-cmark pipeline. Drops zero-width lines (matches the empty-line
/// filtering done at every existing caller site).
pub fn render_markdown_lines(text: &str) -> Vec<ContentLine> {
    let theme = TuiTheme::default();
    project_markdown_to_lines(text)
        .into_iter()
        .filter(|line| line.width() > 0)
        .map(|line| line_to_content_line(&line, &theme))
        .collect()
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
