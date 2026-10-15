use super::*;

#[test]
fn markdown_projection_fixture_supported_basics_renders_lines_and_inline_styles() {
    let markdown = markdown_fixture("supported_basics.md");
    let lines = project_markdown_to_lines(&markdown, None);
    let rendered = lines.iter().map(plain_line).collect::<Vec<_>>();

    assert_eq!(rendered, vec!["Title", "", "Paragraph with em strong code"]);
    assert!(
        lines[0]
            .spans
            .iter()
            .all(|span| span.hint == StyleHint::MdBold)
    );

    let body = &lines[2].spans;
    assert!(
        body.iter()
            .any(|span| span.text == "em" && span.hint == StyleHint::MdItalic)
    );
    assert!(
        body.iter()
            .any(|span| span.text == "strong" && span.hint == StyleHint::MdBold)
    );
    assert!(
        body.iter()
            .any(|span| { span.text == "code" && span.hint == StyleHint::MdInlineCode })
    );
}

#[test]
fn markdown_projection_fixture_lists_and_blockquote_render_deterministically() {
    let markdown = markdown_fixture("lists_blockquote.md");
    let rendered = plain_lines(&markdown);

    assert_eq!(
        rendered,
        vec![
            "• one",
            "• two",
            "",
            "1. first",
            "2. second",
            "",
            "│ quoted",
            "│ second"
        ]
    );

    // Hanging indent: the list marker width feeds ContentLine.hang_indent so
    // wrapped continuations align under the item text (task 7bd175d2).
    let hang = project_markdown_to_lines(&markdown, None);
    assert_eq!(hang[0].hang_indent, 2, "bullet marker '• ' is 2 cols");
    assert_eq!(hang[1].hang_indent, 2);
    assert_eq!(hang[2].hang_indent, 0, "blank separator has no hang indent");
    assert_eq!(hang[3].hang_indent, 3, "ordered marker '1. ' is 3 cols");
    assert_eq!(hang[4].hang_indent, 3);
    assert_eq!(hang[5].hang_indent, 0);
    assert_eq!(hang[6].hang_indent, 0, "blockquote line is not a list item");
}

#[test]
fn markdown_projection_fixture_fenced_code_blocks_render_with_and_without_language() -> Result<()> {
    let markdown = markdown_fixture("fenced_code_blocks.md");
    let lines = project_markdown_to_lines(&markdown, None);
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
        .ok_or("rust line should be present")?;
    assert!(
        line_has_code_hint(rust_signature, StyleHint::MdCodeKeyword),
        "rust keyword should map to syntax keyword hint"
    );

    let json_line = lines
        .iter()
        .find(|line| plain_line(line).contains("\"nu-agent\""))
        .ok_or("json line should be present")?;
    assert!(
        line_has_code_hint(json_line, StyleHint::MdCodeString),
        "json string token should map to syntax string hint"
    );

    let bash_line = lines
        .iter()
        .find(|line| plain_line(line) == "    echo \"hello\"")
        .ok_or("bash line should be present")?;
    assert!(line_has_non_default_token_style(bash_line));

    let sh_line = lines
        .iter()
        .find(|line| plain_line(line) == "    printf '%s\\n' \"$HOME\"")
        .ok_or("sh line should be present")?;
    assert!(line_has_non_default_token_style(sh_line));

    let yaml_line = lines
        .iter()
        .find(|line| plain_line(line) == "    enabled: true")
        .ok_or("yaml line should be present")?;
    assert!(line_has_non_default_token_style(yaml_line));

    let toml_line = lines
        .iter()
        .find(|line| plain_line(line) == "    count = 3")
        .ok_or("toml line should be present")?;
    if adapter_supports_colored_tokens("toml", "count = 3") {
        assert!(line_has_non_default_token_style(toml_line));
    } else {
        assert!(
            toml_line
                .spans
                .iter()
                .all(|span| matches!(span.hint, StyleHint::Normal | StyleHint::MdCodePlain)),
            "unsupported adapter language should fallback to plain code hint"
        );
    }

    let python_line = lines
        .iter()
        .find(|line| plain_line(line) == "    def greet(name):")
        .ok_or("python line should be present")?;
    assert!(
        line_has_code_hint(python_line, StyleHint::MdCodeFunction),
        "python function token should map to syntax function hint"
    );

    let unknown_line = lines
        .iter()
        .find(|line| plain_line(line) == "    plain block")
        .ok_or("unknown language fallback line should be present")?;
    assert!(
        unknown_line
            .spans
            .iter()
            .all(|span| matches!(span.hint, StyleHint::Normal | StyleHint::MdCodePlain)),
        "unknown language fences should fallback to plain code hint"
    );
    Ok(())
}

/// Sanitization and fallback: pseudo `[code:*]` tags and `<system-reminder>`
/// control blocks are neutralized, valid fences survive, unsupported
/// constructs degrade to readable text, and malformed input does not panic.
#[test]
fn markdown_projection_sanitize_and_fallback_table() -> Result<()> {
    // -- Setup & Fixtures
    let pseudo = "Before\n[code:rust]\nfn main() {}\n[/code]\nAfter";
    let reminder = "Hello\n<system-reminder>do not show this</system-reminder>\nWorld";
    let fences = "```rust\nfn main() {}\n```\n<system-reminder>secret</system-reminder>";

    // -- Exec
    let pseudo_lines = plain_lines(pseudo);
    let reminder_lines = plain_lines(reminder);
    let fence_lines = plain_lines(fences);

    // -- Check: pseudo [code:*] markers are sanitized but content survives.
    assert!(pseudo_lines.iter().any(|line| line == "Before"));
    assert!(
        pseudo_lines
            .iter()
            .any(|line| line.contains("fn main() {}"))
    );
    assert!(pseudo_lines.iter().any(|line| line == "After"));
    assert!(
        !pseudo_lines.iter().any(|line| line.contains("[code:")),
        "pseudo [code:*] markers must not appear in projected output"
    );
    assert!(
        !pseudo_lines.iter().any(|line| line.contains("[/code]")),
        "closing [/code] marker must not appear in projected output"
    );

    // -- Check: system-reminder control blocks are neutralized.
    assert!(reminder_lines.iter().any(|line| line == "Hello"));
    assert!(reminder_lines.iter().any(|line| line == "World"));
    assert!(
        !reminder_lines
            .iter()
            .any(|line| line.contains("<system-reminder>")),
        "raw control tag must not appear in projected output"
    );
    assert!(
        !reminder_lines
            .iter()
            .any(|line| line.contains("do not show this")),
        "control block content must be neutralized in projected output"
    );

    // -- Check: valid fences survive while control markers are removed.
    assert!(
        fence_lines.iter().any(|line| line.contains("fn main() {}")),
        "valid fenced markdown content must still render"
    );
    assert!(
        !fence_lines
            .iter()
            .any(|line| line.contains("<system-reminder>")),
        "control markers should be removed without breaking fences"
    );

    // -- Check: unsupported constructs fall back to readable text.
    let unsupported = plain_lines(&markdown_fixture("unsupported_fallback.md"));
    assert!(
        unsupported
            .iter()
            .any(|line| line.contains("<details><summary>Title</summary>Body</details>"))
    );
    assert!(
        unsupported
            .iter()
            .any(|line| line.contains("alt (image: https://img.example/x.png)"))
    );
    assert!(
        unsupported
            .iter()
            .any(|line| line.contains("col") && line.contains("val"))
    );
    assert!(
        unsupported
            .iter()
            .any(|line| line.contains("a") && line.contains("b"))
    );
    assert!(
        unsupported.iter().any(|line| line.contains("│")),
        "table cells should be separated"
    );

    // -- Check: malformed markdown does not panic and stays readable.
    let malformed = markdown_fixture("malformed.md");
    let result = std::panic::catch_unwind(|| plain_lines(&malformed));
    assert!(result.is_ok(), "projection panicked for malformed fixture");
    let lines = result.map_err(|_| "catch_unwind should return projected lines")?;
    let joined = lines.join("\n");
    assert!(!joined.trim().is_empty());
    assert!(joined.contains("fn main() {"));
    assert!(joined.contains("{\"broken\":"));
    Ok(())
}
