use ratatui::style::Style;

use crate::agent::ui::tui::rendering::highlight::{
    HighlightRequest, SyntaxTokenChannel, highlight_source_tokens,
};
use crate::agent::ui::tui::rendering::theme::TuiTheme;

#[derive(Debug, Clone)]
pub(super) struct CodeBlockState {
    pub(super) language_hint: Option<String>,
    pub(super) source: String,
}

pub(super) fn fence_language_hint(kind: pulldown_cmark::CodeBlockKind<'_>) -> Option<String> {
    match kind {
        pulldown_cmark::CodeBlockKind::Fenced(label) => {
            let label = label
                .split_ascii_whitespace()
                .next()
                .unwrap_or_default()
                .trim();
            if label.is_empty() {
                None
            } else {
                Some(label.to_string())
            }
        }
        pulldown_cmark::CodeBlockKind::Indented => None,
    }
}

pub(super) fn highlighted_code_lines(
    block: &CodeBlockState,
    theme: &TuiTheme,
) -> Vec<Vec<(String, Style)>> {
    if block.source.is_empty() {
        return Vec::new();
    }

    highlight_source_tokens(HighlightRequest {
        language_hint: block.language_hint.as_deref(),
        source: &block.source,
    })
    .into_iter()
    .map(|token_line| {
        token_line
            .into_iter()
            .map(|token| {
                let style = style_for_channel(theme, token.channel);
                (token.text, style)
            })
            .collect::<Vec<_>>()
    })
    .collect::<Vec<_>>()
}

fn style_for_channel(theme: &TuiTheme, channel: SyntaxTokenChannel) -> Style {
    match channel {
        SyntaxTokenChannel::Keyword => theme.syntax_keyword,
        SyntaxTokenChannel::Type => theme.syntax_type,
        SyntaxTokenChannel::Function => theme.syntax_function,
        SyntaxTokenChannel::Variable => theme.syntax_variable,
        SyntaxTokenChannel::Constant => theme.syntax_constant,
        SyntaxTokenChannel::String => theme.syntax_string,
        SyntaxTokenChannel::Number => theme.syntax_number,
        SyntaxTokenChannel::Operator => theme.syntax_operator,
        SyntaxTokenChannel::Punctuation => theme.syntax_punctuation,
        SyntaxTokenChannel::Comment => theme.syntax_comment,
        SyntaxTokenChannel::Plain => Style::default(),
    }
}
