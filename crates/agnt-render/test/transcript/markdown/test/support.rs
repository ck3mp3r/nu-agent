//! Shared fixtures for the markdown projection test modules.
//!
//! Holds the imports, the `Result` alias, and the line-inspection helpers
//! every topical markdown test file needs. Sub-files pull them in with
//! `use super::*;`.

pub(super) use crate::test_support::markdown_fixture;
pub(super) use crate::transcript::highlight::{
    HighlightRequest, SyntaxTokenChannel, highlight_source_tokens,
};
pub(super) use crate::transcript::ir::StyleHint;
pub(super) use crate::transcript::ir::StyleHint as IrStyleHint;
pub(super) use crate::transcript::ir::{ContentLine, DiffTint};
pub(super) use crate::transcript::markdown::render_markdown_lines;
pub(super) use crate::transcript::markdown::unwrap_single_fenced_block;
pub(super) use crate::transcript::markdown::{project_diff_lines, project_markdown_to_lines};

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

pub(super) fn plain_line(line: &crate::transcript::ir::ContentLine) -> String {
    line.spans
        .iter()
        .map(|span| span.text.as_str())
        .collect::<String>()
}

pub(super) fn plain_lines(markdown: &str) -> Vec<String> {
    project_markdown_to_lines(markdown, None)
        .into_iter()
        .map(|line| plain_line(&line))
        .collect::<Vec<_>>()
}

pub(super) fn line_has_code_hint(
    line: &crate::transcript::ir::ContentLine,
    hint: StyleHint,
) -> bool {
    line.spans.iter().any(|span| span.hint == hint)
}

pub(super) fn line_has_non_default_token_style(line: &crate::transcript::ir::ContentLine) -> bool {
    line.spans
        .iter()
        .skip_while(|span| span.text == "    ")
        .any(|span| span.hint != StyleHint::Normal)
}

pub(super) fn adapter_supports_colored_tokens(language: &str, source: &str) -> bool {
    highlight_source_tokens(HighlightRequest {
        language_hint: Some(language),
        source,
    })
    .iter()
    .any(|line| {
        line.iter()
            .any(|span| span.channel != SyntaxTokenChannel::Plain)
    })
}

/// The Rust diff fixture used by the projection tests: a file header pair, one
/// hunk header, a context line, a removed line, and an added line.
pub(super) const RUST_DIFF: &str =
    "--- a/a.rs\n+++ b/a.rs\n@@ -1,2 +1,2 @@\n fn main() {\n-let x = 1;\n+let x = 42;\n";

pub(super) fn diff_line<'a>(lines: &'a [ContentLine], needle: &str) -> Result<&'a ContentLine> {
    lines
        .iter()
        .find(|line| plain_line(line).contains(needle))
        .ok_or_else(|| format!("diff line containing {needle:?} should be projected").into())
}
