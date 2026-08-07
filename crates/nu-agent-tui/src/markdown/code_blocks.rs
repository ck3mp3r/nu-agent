use nu_agent_core::transcript::ir::StyleHint;

use crate::rendering::highlight::{HighlightRequest, SyntaxTokenChannel, highlight_source_tokens};

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

pub(super) fn highlighted_code_lines(block: &CodeBlockState) -> Vec<Vec<(String, StyleHint)>> {
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
            .map(|token| (token.text, channel_to_hint(token.channel)))
            .collect::<Vec<_>>()
    })
    .collect::<Vec<_>>()
}

fn channel_to_hint(channel: SyntaxTokenChannel) -> StyleHint {
    match channel {
        SyntaxTokenChannel::Keyword => StyleHint::MdCodeKeyword,
        SyntaxTokenChannel::Type => StyleHint::MdCodeType,
        SyntaxTokenChannel::Function => StyleHint::MdCodeFunction,
        SyntaxTokenChannel::Variable => StyleHint::MdCodeVariable,
        SyntaxTokenChannel::Constant => StyleHint::MdCodeConstant,
        SyntaxTokenChannel::String => StyleHint::MdCodeString,
        SyntaxTokenChannel::Number => StyleHint::MdCodeNumber,
        SyntaxTokenChannel::Operator => StyleHint::MdCodeOperator,
        SyntaxTokenChannel::Punctuation => StyleHint::MdCodePunctuation,
        SyntaxTokenChannel::Comment => StyleHint::MdCodeComment,
        SyntaxTokenChannel::Plain => StyleHint::MdCodePlain,
    }
}
