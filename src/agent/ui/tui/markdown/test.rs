use ratatui::{
    style::{Color, Modifier},
    text::Line,
};

use crate::agent::ui::tui::{
    rendering::highlight::{HighlightRequest, SyntaxTokenChannel, highlight_source_tokens},
    markdown::project_markdown_to_lines,
    test_support::markdown_fixture,
};

const CTP_MOCHA_YELLOW: Color = Color::Rgb(249, 226, 175);
const CTP_MOCHA_GREEN: Color = Color::Rgb(166, 227, 161);
const CTP_MOCHA_BLUE: Color = Color::Rgb(137, 180, 250);
const CTP_MOCHA_MAUVE: Color = Color::Rgb(203, 166, 247);

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

fn line_has_color(line: &Line<'_>, color: Color) -> bool {
    line.spans.iter().any(|span| span.style.fg == Some(color))
}

fn line_has_non_default_token_style(line: &Line<'_>) -> bool {
    line.spans
        .iter()
        .skip_while(|span| span.content.as_ref() == "    ")
        .any(|span| span.style != Default::default())
}

fn adapter_supports_colored_tokens(language: &str, source: &str) -> bool {
    highlight_source_tokens(HighlightRequest {
        language_hint: Some(language),
        source,
    })
    .iter()
    .any(|line| line.iter().any(|span| span.channel != SyntaxTokenChannel::Plain))
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
        .any(|span| {
            span.content.as_ref() == "code"
                && span.style.fg == Some(CTP_MOCHA_YELLOW)
                && span.style.add_modifier.contains(Modifier::DIM)
        }));
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
    let lines = project_markdown_to_lines(&markdown);
    let rendered = lines.iter().map(plain_line).collect::<Vec<_>>();

    assert!(rendered.contains(&"    fn main() {".to_string()));
    assert!(rendered.contains(&"        let answer: i32 = 42;".to_string()));
    assert!(rendered.contains(&"    {\"name\":\"nu-agent\",\"ok\":true}".to_string()));
    assert!(rendered.contains(&"    echo \"hello\"".to_string()));
    assert!(rendered.contains(&"    printf '%s\\n' \"$HOME\"".to_string()));
    assert!(rendered.contains(&"    name: nu-agent".to_string()));
    assert!(rendered.contains(&"    name = \"nu-agent\"".to_string()));
    assert!(rendered.contains(&"    def greet(name):".to_string()));
    assert!(rendered.contains(&"    plain block".to_string()));
    assert!(
        !rendered.iter().any(|line| line.starts_with("[code:")),
        "projected output must not leak bracket code pseudo-tags"
    );

    let rust_signature = lines
        .iter()
        .find(|line| plain_line(line) == "    fn main() {")
        .expect("rust line should be present");
    assert!(
        line_has_color(rust_signature, CTP_MOCHA_MAUVE),
        "rust keyword should map to syntax keyword color"
    );

    let json_line = lines
        .iter()
        .find(|line| plain_line(line).contains("\"nu-agent\""))
        .expect("json line should be present");
    assert!(
        line_has_color(json_line, CTP_MOCHA_GREEN),
        "json string token should map to syntax string color"
    );

    let bash_line = lines
        .iter()
        .find(|line| plain_line(line) == "    echo \"hello\"")
        .expect("bash line should be present");
    assert!(line_has_non_default_token_style(bash_line));

    let sh_line = lines
        .iter()
        .find(|line| plain_line(line) == "    printf '%s\\n' \"$HOME\"")
        .expect("sh line should be present");
    assert!(line_has_non_default_token_style(sh_line));

    let yaml_line = lines
        .iter()
        .find(|line| plain_line(line) == "    enabled: true")
        .expect("yaml line should be present");
    assert!(line_has_non_default_token_style(yaml_line));

    let toml_line = lines
        .iter()
        .find(|line| plain_line(line) == "    count = 3")
        .expect("toml line should be present");
    if adapter_supports_colored_tokens("toml", "count = 3") {
        assert!(line_has_non_default_token_style(toml_line));
    } else {
        assert!(
            toml_line
                .spans
                .iter()
                .all(|span| span.style == Default::default()),
            "unsupported adapter language should fallback to plain code style"
        );
    }

    let python_line = lines
        .iter()
        .find(|line| plain_line(line) == "    def greet(name):")
        .expect("python line should be present");
    assert!(
        line_has_color(python_line, CTP_MOCHA_BLUE),
        "python function token should map to syntax function color"
    );

    let unknown_line = lines
        .iter()
        .find(|line| plain_line(line) == "    plain block")
        .expect("unknown language fallback line should be present");
    assert!(
        unknown_line
            .spans
            .iter()
            .all(|span| span.style == Default::default()),
        "unknown language fences should fallback to plain code style"
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
    assert!(joined.contains("{\"broken\": "));
}

#[test]
fn markdown_projection_sanitizes_pseudo_code_tags_from_visible_output() {
    let markdown = "Before\n[code:rust]\nfn main() {}\n[/code]\nAfter";
    let rendered = plain_lines(markdown);

    assert!(rendered.iter().any(|line| line == "Before"));
    assert!(rendered.iter().any(|line| line.contains("fn main() {}")));
    assert!(rendered.iter().any(|line| line == "After"));
    assert!(
        !rendered.iter().any(|line| line.contains("[code:")),
        "pseudo [code:*] markers must not appear in projected output"
    );
    assert!(
        !rendered.iter().any(|line| line.contains("[/code]")),
        "closing [/code] marker must not appear in projected output"
    );
}

#[test]
fn markdown_projection_sanitizes_system_reminder_control_blocks() {
    let markdown = "Hello\n<system-reminder>do not show this</system-reminder>\nWorld";
    let rendered = plain_lines(markdown);

    assert!(rendered.iter().any(|line| line == "Hello"));
    assert!(rendered.iter().any(|line| line == "World"));
    assert!(
        !rendered.iter().any(|line| line.contains("<system-reminder>")),
        "raw control tag must not appear in projected output"
    );
    assert!(
        !rendered.iter().any(|line| line.contains("do not show this")),
        "control block content must be neutralized in projected output"
    );
}

#[test]
fn markdown_projection_preserves_valid_markdown_fences_while_sanitizing_control_markers() {
    let markdown = "```rust\nfn main() {}\n```\n<system-reminder>secret</system-reminder>";
    let rendered = plain_lines(markdown);

    assert!(
        rendered.iter().any(|line| line.contains("fn main() {}")),
        "valid fenced markdown content must still render"
    );
    assert!(
        !rendered.iter().any(|line| line.contains("<system-reminder>")),
        "control markers should be removed without breaking fences"
    );
}
