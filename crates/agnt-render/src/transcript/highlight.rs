use std::sync::OnceLock;

use syntect::{
    easy::ScopeRegionIterator,
    parsing::{ParseState, ScopeStack, SyntaxReference, SyntaxSet},
    util::LinesWithEndings,
};

use super::syntax_packdump::{self, SyntaxSetSpec};

pub(crate) static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();

pub(crate) fn cached_syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(|| syntax_packdump::build_syntax_set(&SyntaxSetSpec::Default))
}

#[derive(Debug, Clone, Copy)]
pub struct HighlightRequest<'a> {
    pub language_hint: Option<&'a str>,
    pub source: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntaxTokenChannel {
    Keyword,
    Type,
    Function,
    Variable,
    Constant,
    String,
    Number,
    Operator,
    Punctuation,
    Comment,
    Plain,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightedSpan {
    pub text: String,
    pub channel: SyntaxTokenChannel,
}

/// The syntax `hint` names in `syntax_set`, by token, name or extension.
///
/// A hidden syntax never resolves: the configured language list hides every
/// syntax it leaves out, and the hidden flag is what keeps them unreachable.
pub(crate) fn resolve_syntax_hint<'a>(
    syntax_set: &'a SyntaxSet,
    language_hint: &str,
) -> Option<&'a SyntaxReference> {
    let hint = language_hint.trim();
    if hint.is_empty() {
        return None;
    }

    syntax_set
        .find_syntax_by_token(hint)
        .or_else(|| syntax_set.find_syntax_by_name(hint))
        .or_else(|| syntax_set.find_syntax_by_extension(hint))
        .or_else(|| {
            syntax_set
                .syntaxes()
                .iter()
                .find(|syntax| syntax.name.eq_ignore_ascii_case(hint))
        })
        .or_else(|| {
            syntax_set.syntaxes().iter().find(|syntax| {
                syntax
                    .file_extensions
                    .iter()
                    .any(|ext| ext.eq_ignore_ascii_case(hint))
            })
        })
        .filter(|syntax| !syntax.hidden)
}

/// The highlighted span lines `request` produces against `syntax_set`.
pub(crate) fn highlight_with_syntax_set(
    request: HighlightRequest<'_>,
    syntax_set: &SyntaxSet,
) -> Vec<Vec<HighlightedSpan>> {
    if request.source.is_empty() {
        return Vec::new();
    }

    let canonical_hint = canonical_language_hint(request.language_hint);
    match canonical_hint
        .as_deref()
        .and_then(|hint| resolve_syntax_hint(syntax_set, hint))
    {
        Some(syntax) => highlight_with_syntax(request.source, syntax_set, syntax),
        None => plain_token_lines(request.source),
    }
}

fn highlight_with_syntax(
    source: &str,
    syntax_set: &SyntaxSet,
    syntax: &SyntaxReference,
) -> Vec<Vec<HighlightedSpan>> {
    let mut parse_state = ParseState::new(syntax);
    let mut scope_stack = ScopeStack::new();
    let mut lines = Vec::new();
    for raw_line in normalized_lines(source) {
        let line = raw_line.as_str();
        let line_with_newline = format!("{line}\n");
        let parsed = parse_state.parse_line(&line_with_newline, syntax_set);
        match parsed {
            Ok(parsed_line) => {
                let mut spans = Vec::new();
                for (region, op) in ScopeRegionIterator::new(&parsed_line, &line_with_newline) {
                    if scope_stack.apply(op).is_err() {
                        spans.clear();
                        spans.push(HighlightedSpan {
                            text: line.to_string(),
                            channel: SyntaxTokenChannel::Plain,
                        });
                        break;
                    }

                    if region.is_empty() {
                        continue;
                    }

                    if region == "\n" {
                        continue;
                    }

                    spans.push(HighlightedSpan {
                        text: region.to_string(),
                        channel: classify_scope_channel(&scope_stack),
                    });
                }
                lines.push(spans);
            }
            Err(_) => {
                lines.push(vec![HighlightedSpan {
                    text: line.to_string(),
                    channel: SyntaxTokenChannel::Plain,
                }]);
            }
        }
    }
    lines
}

fn canonical_language_hint(language_hint: Option<&str>) -> Option<String> {
    let hint = language_hint?.trim().to_ascii_lowercase();
    if hint.is_empty() {
        return None;
    }

    let mapped = match hint.as_str() {
        "javascript" | "node" => "js",
        "typescript" => "ts",
        "shell" | "console" | "terminal" => "bash",
        "zsh" => "bash",
        "nushell" => "nu",
        _ => hint.as_str(),
    };
    Some(mapped.to_string())
}

fn classify_scope_channel(scope_stack: &ScopeStack) -> SyntaxTokenChannel {
    let scope_strings = scope_stack
        .as_slice()
        .iter()
        .map(|scope| scope.build_string().to_ascii_lowercase())
        .collect::<Vec<_>>();

    if scope_strings.is_empty() {
        return SyntaxTokenChannel::Plain;
    }

    if scope_has_any(&scope_strings, &["comment"]) {
        return SyntaxTokenChannel::Comment;
    }

    if scope_has_any(
        &scope_strings,
        &[
            "keyword.operator",
            "keyword.control.operator",
            "punctuation.definition.operator",
        ],
    ) {
        return SyntaxTokenChannel::Operator;
    }

    if scope_has_any(
        &scope_strings,
        &[
            "punctuation",
            "meta.brace",
            "meta.delimiter",
            "meta.group",
            "meta.separator",
            "meta.parens",
            "meta.brackets",
        ],
    ) {
        return SyntaxTokenChannel::Punctuation;
    }

    if scope_has_any(&scope_strings, &["string"]) {
        return SyntaxTokenChannel::String;
    }

    if scope_has_any(
        &scope_strings,
        &[
            "constant.numeric",
            "number",
            "constant.character.escape",
            "constant.language.boolean",
        ],
    ) {
        return SyntaxTokenChannel::Number;
    }

    if scope_has_any(
        &scope_strings,
        &[
            "constant",
            "support.constant",
            "variable.language",
            "entity.name.constant",
        ],
    ) {
        return SyntaxTokenChannel::Constant;
    }

    if scope_has_any(
        &scope_strings,
        &[
            "entity.name.function",
            "support.function",
            "variable.function",
        ],
    ) {
        return SyntaxTokenChannel::Function;
    }

    if scope_has_any(
        &scope_strings,
        &[
            "entity.name.type",
            "support.type",
            "support.class",
            "support.struct",
            "support.enum",
        ],
    ) {
        return SyntaxTokenChannel::Type;
    }

    if scope_has_any(
        &scope_strings,
        &[
            "keyword",
            "storage.type",
            "storage.modifier",
            "storage.control",
            "storage.keyword",
            "storage.function",
        ],
    ) {
        return SyntaxTokenChannel::Keyword;
    }

    if scope_has_any(
        &scope_strings,
        &[
            "variable",
            "entity.name.variable",
            "meta.definition.variable",
            "meta.assignment",
            "entity.other.attribute-name",
            "entity.other.property-name",
        ],
    ) {
        return SyntaxTokenChannel::Variable;
    }

    SyntaxTokenChannel::Plain
}

fn scope_has_any(scope_strings: &[String], needles: &[&str]) -> bool {
    scope_strings
        .iter()
        .any(|scope| needles.iter().any(|needle| scope.contains(needle)))
}

fn normalized_lines(source: &str) -> Vec<String> {
    let normalized = source.replace("\r\n", "\n").replace('\r', "\n");
    let mut result = Vec::new();
    for raw in LinesWithEndings::from(&normalized) {
        let trimmed = raw.strip_suffix('\n').unwrap_or(raw);
        result.push(trimmed.to_string());
    }
    if result.is_empty() {
        result.push(normalized);
    }
    result
}

fn plain_token_lines(source: &str) -> Vec<Vec<HighlightedSpan>> {
    normalized_lines(source)
        .into_iter()
        .map(|line| {
            vec![HighlightedSpan {
                text: line,
                channel: SyntaxTokenChannel::Plain,
            }]
        })
        .collect::<Vec<_>>()
}

/// The highlighted span lines `request` produces against the process-wide
/// syntax set.
pub fn highlight_source_tokens(request: HighlightRequest<'_>) -> Vec<Vec<HighlightedSpan>> {
    let syntax_set = cached_syntax_set();
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        highlight_with_syntax_set(request, syntax_set)
    }))
    .unwrap_or_else(|_| plain_token_lines(request.source))
}
