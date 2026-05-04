use std::path::PathBuf;

use ratatui::{
    style::{Color, Modifier},
    text::Line,
};

use crate::commands::agent::ui::tui::markdown::project_markdown_to_lines;

fn markdown_fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src/commands/agent/ui/tui/fixtures/markdown")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("failed to read markdown fixture {}: {error}", path.display())
    })
}

fn plain_line(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>()
}

fn plain_lines(markdown: &str) -> Vec<String> {
    project_markdown_to_lines(markdown)
        .into_iter()
        .map(|line| plain_line(&line))
        .collect::<Vec<_>>()
}

#[test]
fn markdown_projection_fixture_supported_basics_renders_lines_and_inline_styles() {
    let markdown = markdown_fixture("supported_basics.md");
    let lines = project_markdown_to_lines(&markdown);
    let rendered = lines.iter().map(plain_line).collect::<Vec<_>>();

    assert_eq!(rendered, vec!["Title", "Paragraph with em strong code"]);
    assert!(lines[0].spans.iter().all(|span| span.style.add_modifier.contains(Modifier::BOLD)));

    let body = &lines[1].spans;
    assert!(body
        .iter()
        .any(|span| span.content.as_ref() == "em" && span.style.add_modifier.contains(Modifier::ITALIC)));
    assert!(body
        .iter()
        .any(|span| span.content.as_ref() == "strong" && span.style.add_modifier.contains(Modifier::BOLD)));
    assert!(body
        .iter()
        .any(|span| span.content.as_ref() == "code" && span.style.bg == Some(Color::DarkGray)));
}

#[test]
fn markdown_projection_fixture_lists_and_blockquote_render_deterministically() {
    let markdown = markdown_fixture("lists_blockquote.md");
    let rendered = plain_lines(&markdown);

    assert_eq!(
        rendered,
        vec!["• one", "• two", "1. first", "2. second", "│ quoted", "│ second"]
    );
}

#[test]
fn markdown_projection_fixture_fenced_code_blocks_render_with_and_without_language() {
    let markdown = markdown_fixture("fenced_code_blocks.md");
    let rendered = plain_lines(&markdown);

    assert_eq!(
        rendered,
        vec!["[code:rust]", "    fn main() {}", "    plain block"]
    );
}

#[test]
fn markdown_projection_fixture_unsupported_constructs_have_readable_fallback() {
    let markdown = markdown_fixture("unsupported_fallback.md");
    let rendered = plain_lines(&markdown);

    assert!(rendered
        .iter()
        .any(|line| line.contains("<details><summary>Title</summary>Body</details>")));
    assert!(rendered
        .iter()
        .any(|line| line.contains("alt (image: https://img.example/x.png)")));
    assert!(rendered.iter().any(|line| line.contains("| col | val |")));
    assert!(rendered.iter().any(|line| line.contains("| a | b |")));
}

#[test]
fn markdown_projection_fixture_malformed_markdown_does_not_panic_and_remains_readable() {
    let markdown = markdown_fixture("malformed.md");

    let result = std::panic::catch_unwind(|| plain_lines(&markdown));
    assert!(result.is_ok(), "projection panicked for malformed fixture");

    let lines = result.expect("catch_unwind should return projected lines");
    let joined = lines.join("\n");
    assert!(!joined.trim().is_empty());
    assert!(joined.contains("fn main() {"));
}
