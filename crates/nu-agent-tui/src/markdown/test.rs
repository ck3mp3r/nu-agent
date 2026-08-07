use nu_agent_core::transcript::ir::StyleHint;

use crate::{
    markdown::project_markdown_to_lines,
    rendering::highlight::{HighlightRequest, SyntaxTokenChannel, highlight_source_tokens},
    test_support::markdown_fixture,
};

fn plain_line(line: &nu_agent_core::transcript::ir::ContentLine) -> String {
    line.spans
        .iter()
        .map(|span| span.text.as_str())
        .collect::<String>()
}

fn plain_lines(markdown: &str) -> Vec<String> {
    project_markdown_to_lines(markdown, None)
        .into_iter()
        .map(|line| plain_line(&line))
        .collect::<Vec<_>>()
}

fn line_has_code_hint(line: &nu_agent_core::transcript::ir::ContentLine, hint: StyleHint) -> bool {
    line.spans.iter().any(|span| span.hint == hint)
}

fn line_has_non_default_token_style(line: &nu_agent_core::transcript::ir::ContentLine) -> bool {
    line.spans
        .iter()
        .skip_while(|span| span.text == "    ")
        .any(|span| span.hint != StyleHint::Normal)
}

fn adapter_supports_colored_tokens(language: &str, source: &str) -> bool {
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
}

#[test]
fn markdown_projection_fixture_fenced_code_blocks_render_with_and_without_language() {
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
        .expect("rust line should be present");
    assert!(
        line_has_code_hint(rust_signature, StyleHint::MdCodeKeyword),
        "rust keyword should map to syntax keyword hint"
    );

    let json_line = lines
        .iter()
        .find(|line| plain_line(line).contains("\"nu-agent\""))
        .expect("json line should be present");
    assert!(
        line_has_code_hint(json_line, StyleHint::MdCodeString),
        "json string token should map to syntax string hint"
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
                .all(|span| matches!(span.hint, StyleHint::Normal | StyleHint::MdCodePlain)),
            "unsupported adapter language should fallback to plain code hint"
        );
    }

    let python_line = lines
        .iter()
        .find(|line| plain_line(line) == "    def greet(name):")
        .expect("python line should be present");
    assert!(
        line_has_code_hint(python_line, StyleHint::MdCodeFunction),
        "python function token should map to syntax function hint"
    );

    let unknown_line = lines
        .iter()
        .find(|line| plain_line(line) == "    plain block")
        .expect("unknown language fallback line should be present");
    assert!(
        unknown_line
            .spans
            .iter()
            .all(|span| matches!(span.hint, StyleHint::Normal | StyleHint::MdCodePlain)),
        "unknown language fences should fallback to plain code hint"
    );
}

#[test]
fn markdown_projection_fixture_unsupported_constructs_have_readable_fallback() {
    let markdown = markdown_fixture("unsupported_fallback.md");
    let rendered = plain_lines(&markdown);

    assert!(
        rendered
            .iter()
            .any(|line| line.contains("<details><summary>Title</summary>Body</details>"))
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.contains("alt (image: https://img.example/x.png)"))
    );
    // Tables are now supported and rendered with separators
    assert!(
        rendered
            .iter()
            .any(|line| line.contains("col") && line.contains("val"))
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.contains("a") && line.contains("b"))
    );
    assert!(
        rendered.iter().any(|line| line.contains("│")),
        "table cells should be separated"
    );
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
    assert!(joined.contains("{\"broken\":"));
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
        !rendered
            .iter()
            .any(|line| line.contains("<system-reminder>")),
        "raw control tag must not appear in projected output"
    );
    assert!(
        !rendered
            .iter()
            .any(|line| line.contains("do not show this")),
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
        !rendered
            .iter()
            .any(|line| line.contains("<system-reminder>")),
        "control markers should be removed without breaking fences"
    );
}

#[test]
fn markdown_projection_renders_table_with_separator_and_bold_header() {
    let markdown =
        "| Commit | Message |\n|--------|--------|\n| abc123 | fix bug |\n| def456 | add feature |";
    let lines = project_markdown_to_lines(markdown, None);
    let plain: Vec<String> = lines.iter().map(plain_line).collect();

    assert!(
        plain
            .iter()
            .any(|l| l.contains("Commit") && l.contains("Message")),
        "header row should contain column names, got: {plain:?}"
    );
    assert!(
        plain.iter().any(|l| l.contains("─") && l.contains("┼")),
        "separator row should contain horizontal lines with junction, got: {plain:?}"
    );
    assert!(
        plain
            .iter()
            .any(|l| l.contains("abc123") && l.contains("fix bug")),
        "data row should contain cell values, got: {plain:?}"
    );

    // Header text should be bold
    let header_line = lines
        .iter()
        .find(|l| {
            let text = plain_line(l);
            text.contains("Commit")
        })
        .expect("header line exists");
    let has_bold = header_line
        .spans
        .iter()
        .any(|s| s.hint == StyleHint::MdBold);
    assert!(has_bold, "header cells should be bold");
}

#[test]
fn markdown_projection_renders_table_with_aligned_columns() {
    let markdown = "| A | Long Header |\n|---|---|\n| x | y |\n| longer text | z |";
    let lines = project_markdown_to_lines(markdown, None);
    let plain: Vec<String> = lines.iter().map(plain_line).collect();

    // Find separator line
    let sep = plain
        .iter()
        .find(|l| l.contains("┼"))
        .expect("separator line exists");
    // Separator parts should have different widths matching column content
    let parts: Vec<&str> = sep.split('┼').collect();
    assert_eq!(parts.len(), 2, "two columns = two separator parts");
    assert!(
        parts[0].len() >= " longer text ".len(),
        "first separator should be at least as wide as longest cell: {:?}",
        parts[0]
    );
}

#[test]
fn markdown_projection_table_separator_intersections_align_with_header_bars() {
    // Locks in the alignment between header `│` column-separators and separator `┼`
    // intersections. With rounded borders the edges use `├`/`┤` (separator) and
    // `│` (content rows), so we compare only the inter-column positions — skipping
    // the outermost edge characters.
    let markdown = "| # | Hash    | Date         | Author             | Message |\n\
                    |---|---------|--------------|--------------------|---------|\n\
                    | 0 | 66f1401 | 2026-06-18   | Christian Kemper   | feat(tui): branch icon |";
    let lines = project_markdown_to_lines(markdown, None);
    let plain: Vec<String> = lines.iter().map(plain_line).collect();

    let header = plain
        .iter()
        .find(|l| l.contains("Hash") && l.contains("Author"))
        .expect("header row exists");
    let separator = plain
        .iter()
        .find(|l| l.contains("┼"))
        .expect("separator row exists");

    // Header: leading `│` + inter-column `│`s + trailing `│`.
    // We want the inter-column positions only (skip first and last).
    let bar_positions: Vec<usize> = {
        let all: Vec<usize> = header
            .char_indices()
            .filter(|(_, c)| *c == '│')
            .map(|(i, _)| header[..i].chars().count())
            .collect();
        // Drop first (left border) and last (right border); keep inter-column bars.
        all[1..all.len().saturating_sub(1)].to_vec()
    };

    // Separator: `├` at left edge, `┼` inter-column, `┤` at right edge.
    let intersection_positions: Vec<usize> = separator
        .char_indices()
        .filter(|(_, c)| *c == '┼')
        .map(|(i, _)| separator[..i].chars().count())
        .collect();

    assert_eq!(
        bar_positions, intersection_positions,
        "every inter-column header `│` must sit at the same column as the matching separator `┼` \
         (got bars at {bar_positions:?}, intersections at {intersection_positions:?})\n\
         header:    {header}\n\
         separator: {separator}"
    );

    // Additionally verify the separator edges use ├ and ┤
    assert!(
        separator.starts_with('├'),
        "separator left edge should be ├, got: {separator}"
    );
    assert!(
        separator.ends_with('┤'),
        "separator right edge should be ┤, got: {separator}"
    );
}

#[test]
fn table_has_rounded_top_border() {
    let markdown = "| A | B |\n|---|---|\n| x | y |";
    let plain = plain_lines(markdown);

    let top = plain.first().expect("table should have at least one line");
    assert!(
        top.contains('╭') && top.contains('╮'),
        "first output line should be the rounded top border with ╭ and ╮, got: {top}"
    );
}

#[test]
fn table_has_rounded_bottom_border() {
    let markdown = "| A | B |\n|---|---|\n| x | y |";
    let plain = plain_lines(markdown);

    let bottom = plain.last().expect("table should have at least one line");
    assert!(
        bottom.contains('╰') && bottom.contains('╯'),
        "last output line should be the rounded bottom border with ╰ and ╯, got: {bottom}"
    );
}

#[test]
fn table_has_left_and_right_borders_on_data_rows() {
    let markdown = "| A | B |\n|---|---|\n| x | y |\n| p | q |";
    let plain = plain_lines(markdown);

    // Content rows (header + data rows, not top/bottom borders) must start and end with │
    let content_rows: Vec<&String> = plain
        .iter()
        .filter(|l| l.contains('A') || l.contains('x') || l.contains('p'))
        .collect();

    assert!(
        !content_rows.is_empty(),
        "should have at least one content row"
    );
    for row in content_rows {
        assert!(
            row.starts_with('│'),
            "content row should start with │, got: {row}"
        );
        assert!(
            row.ends_with('│'),
            "content row should end with │, got: {row}"
        );
    }
}

#[test]
fn table_clamped_when_over_max_width() {
    // Build a 5-column table; choose max_width so only 3 columns fit.
    // Each column header is 1 char wide → col_width = 1.
    // Total for N cols = 1 + 3*N + sum(col_widths) = 1 + 3*N + N*1 = 1 + 4*N.
    // 3 cols → 1 + 4*3 = 13 chars. 4 cols → 1 + 4*4 = 17 chars.
    // Use max_width = 16: fits 3 cols (13 ≤ 16) but not 4 (17 > 16).
    let markdown = "| A | B | C | D | E |\n|---|---|---|---|---|\n| 1 | 2 | 3 | 4 | 5 |";
    let lines = project_markdown_to_lines(markdown, Some(16));
    let plain: Vec<String> = lines.iter().map(plain_line).collect();

    // Every line must be ≤ max_width characters
    for line in &plain {
        assert!(
            line.chars().count() <= 16,
            "line exceeds max_width=16: {line:?} (len={})",
            line.chars().count()
        );
    }

    // The clamped table should have exactly 3 columns:
    // top border: ╭──...─┬──...─┬──...─╮ → 2 ┬ chars
    let top = plain.first().expect("non-empty output");
    let top_joiners = top.chars().filter(|c| *c == '┬').count();
    assert_eq!(
        top_joiners, 2,
        "3-column table top border should have 2 ┬ joiners, got {top_joiners}: {top}"
    );

    // Columns D and E should not appear in any line
    assert!(
        !plain.iter().any(|l| l.contains('D')),
        "column D should have been clamped away"
    );
    assert!(
        !plain.iter().any(|l| l.contains('E')),
        "column E should have been clamped away"
    );
}

#[test]
fn table_always_renders_at_least_one_column() {
    // Even with an impossibly small max_width, at least 1 column must be kept.
    let markdown = "| Alpha | Beta | Gamma |\n|-------|------|-------|\n| a | b | c |";
    let lines = project_markdown_to_lines(markdown, Some(1));
    let plain: Vec<String> = lines.iter().map(plain_line).collect();

    // Should still have a top border
    assert!(
        plain.iter().any(|l| l.contains('╭') && l.contains('╮')),
        "even with max_width=1, should still render top border"
    );

    // The header row should contain "Alpha" (first column)
    assert!(
        plain.iter().any(|l| l.contains("Alpha")),
        "at least the first column (Alpha) must be rendered"
    );
}

#[test]
fn table_clamped_columns_have_correct_right_border() {
    // After clamping, the rightmost border chars should be ╮/┤/╯, not ┬/┼/┴.
    let markdown = "| A | B | C | D | E |\n|---|---|---|---|---|\n| 1 | 2 | 3 | 4 | 5 |";
    // max_width=16 → 3 columns
    let lines = project_markdown_to_lines(markdown, Some(16));
    let plain: Vec<String> = lines.iter().map(plain_line).collect();

    let top = plain.first().expect("has top border");
    assert!(
        top.ends_with('╮'),
        "top border should end with ╮ after clamping, got: {top}"
    );

    let separator = plain
        .iter()
        .find(|l| l.contains('┼') || l.contains('┤'))
        .expect("separator row exists");
    assert!(
        separator.ends_with('┤'),
        "separator should end with ┤ after clamping, got: {separator}"
    );

    let bottom = plain.last().expect("has bottom border");
    assert!(
        bottom.ends_with('╯'),
        "bottom border should end with ╯ after clamping, got: {bottom}"
    );
}

#[test]
fn markdown_projection_renders_table_with_code_in_cells_correctly() {
    let markdown = r#"This is the **nu-agent** project directory. Here's a summary of its contents:

| Name | Type | Description |
|------|------|-------------|
| `AGENTS.md` | file | Agent documentation |
| `Cargo.lock` / `Cargo.toml` | files | Rust project manifest and lockfile |
| `Formula/` | dir | Likely Homebrew formula for distribution |
| `README.md` | file | Project readme |

It's a **Rust project** using **Nix flakes** for development/build environment management."#;
    let lines = project_markdown_to_lines(markdown, None);
    let plain: Vec<String> = lines.iter().map(plain_line).collect();

    // First column values should NOT appear as a concatenated line before the table
    let first_col_values = ["AGENTS.md", "Cargo.lock", "Formula/", "README.md"];
    for line in &plain {
        // Check if this line contains multiple first-column values concatenated
        let matches: Vec<_> = first_col_values
            .iter()
            .filter(|val| line.contains(*val))
            .collect();
        assert!(
            matches.len() <= 1,
            "Line should not contain multiple first-column values concatenated: {:?}\nLine: {}",
            matches,
            line
        );
    }

    // Each data row should contain its Name, Type, and Description separated by │
    let data_rows = [
        ("AGENTS.md", "file", "Agent documentation"),
        ("Cargo.lock", "files", "Rust project manifest"),
        ("Formula/", "dir", "Homebrew formula"),
        ("README.md", "file", "Project readme"),
    ];

    for (name, typ, desc_part) in &data_rows {
        let matching_line = plain
            .iter()
            .find(|line| line.contains(name) && line.contains("│"));

        assert!(
            matching_line.is_some(),
            "Should find a table row containing '{}' with cell separator │",
            name
        );

        let line = matching_line.unwrap();
        assert!(
            line.contains(typ),
            "Row with '{}' should also contain type '{}'\nGot: {}",
            name,
            typ,
            line
        );
        assert!(
            line.contains(desc_part),
            "Row with '{}' should also contain description part '{}'\nGot: {}",
            name,
            desc_part,
            line
        );
    }

    // Table should have header, separator, and data rows
    assert!(
        plain.iter().any(|l| l.contains("Name")
            && l.contains("│")
            && l.contains("Type")
            && l.contains("Description")),
        "Should have header row with Name, Type, Description"
    );
    assert!(
        plain.iter().any(|l| l.contains("─") && l.contains("┼")),
        "Should have separator row"
    );
}

// === render_markdown_lines tests ===

use crate::markdown::render_markdown_lines;
use nu_agent_core::transcript::ir::StyleHint as IrStyleHint;

#[test]
fn render_markdown_lines_empty_input_returns_empty_vec() {
    assert!(render_markdown_lines("", None).is_empty());
}

#[test]
fn render_markdown_lines_plain_text_yields_normal_spans() {
    let lines = render_markdown_lines("hello", None);
    assert_eq!(lines.len(), 1);
    assert!(
        lines[0]
            .spans
            .iter()
            .all(|s| matches!(s.hint, IrStyleHint::Normal))
    );
    let concat: String = lines[0].spans.iter().map(|s| s.text.as_str()).collect();
    assert!(concat.contains("hello"));
}

#[test]
fn render_markdown_lines_bold() {
    let lines = render_markdown_lines("**bold**", None);
    let bold = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .find(|s| matches!(s.hint, IrStyleHint::MdBold))
        .expect("expected MdBold span");
    assert_eq!(bold.text, "bold");
}

#[test]
fn render_markdown_lines_italic() {
    let lines = render_markdown_lines("*italic*", None);
    let italic = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .find(|s| matches!(s.hint, IrStyleHint::MdItalic))
        .expect("expected MdItalic span");
    assert_eq!(italic.text, "italic");
}

#[test]
fn render_markdown_lines_inline_code() {
    let lines = render_markdown_lines("a `code` b", None);
    let code = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .find(|s| matches!(s.hint, IrStyleHint::MdInlineCode))
        .expect("expected MdInlineCode span");
    assert!(code.text.contains("code"));
}

#[test]
fn render_markdown_lines_fenced_code_block() {
    let lines = render_markdown_lines("```rust\nfn x() {}\n```", None);
    assert!(!lines.is_empty());
    let has_code = lines.iter().flat_map(|l| l.spans.iter()).any(|s| {
        matches!(
            s.hint,
            IrStyleHint::MdCodeKeyword
                | IrStyleHint::MdCodeType
                | IrStyleHint::MdCodeFunction
                | IrStyleHint::MdCodeVariable
                | IrStyleHint::MdCodeConstant
                | IrStyleHint::MdCodeString
                | IrStyleHint::MdCodeNumber
                | IrStyleHint::MdCodeOperator
                | IrStyleHint::MdCodePunctuation
                | IrStyleHint::MdCodeComment
                | IrStyleHint::MdCodePlain
                | IrStyleHint::MdInlineCode
        )
    });
    assert!(
        has_code,
        "fenced rust block must produce at least one MdCode* hint"
    );
}

#[test]
fn render_markdown_lines_collapses_consecutive_blank_lines() {
    let lines = render_markdown_lines("first\n\n\nlast", None);
    // Should have: "first", blank, "last" (consecutive blanks collapsed to one)
    assert_eq!(lines.len(), 3);
    assert!(!lines[0].spans.is_empty()); // "first"
    assert!(lines[1].spans.is_empty()); // blank separator
    assert!(!lines[2].spans.is_empty()); // "last"
}

#[test]
fn render_markdown_lines_no_leading_trailing_blanks() {
    let lines = render_markdown_lines("\n\nhello\n\n", None);
    assert_eq!(lines.len(), 1);
    assert!(!lines[0].spans.is_empty());
}

#[test]
fn markdown_table_with_emoji_aligns_right_border_correctly() {
    use unicode_width::UnicodeWidthStr;
    let markdown = "| Name | Status |\n| --- | --- |\n| server | 🟢 |\n| other | ⚪ |\n";
    let lines = plain_lines(markdown);
    let top_border = lines
        .iter()
        .find(|l| l.starts_with('╭'))
        .expect("no top border");
    let border_width = UnicodeWidthStr::width(top_border.as_str());
    for line in &lines {
        if line.contains('│') && !line.contains('─') {
            let row_width = UnicodeWidthStr::width(line.as_str());
            assert_eq!(
                row_width, border_width,
                "row width {} != border width {} for line: {}",
                row_width, border_width, line
            );
        }
    }
}

#[test]
fn render_markdown_lines_tags_code_keyword() {
    let lines = render_markdown_lines("```rust\nfn x() {}\n```", None);
    let has_keyword = lines.iter().flat_map(|l| l.spans.iter()).any(|s| {
        matches!(
            s.hint,
            nu_agent_core::transcript::ir::StyleHint::MdCodeKeyword
        )
    });
    assert!(has_keyword, "should contain MdCodeKeyword span");
}
#[test]
fn inline_math_renders_latex_arrow_as_unicode() {
    let lines = project_markdown_to_lines("model $\\rightarrow$ model", None);
    let text: String = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.text.as_str())
        .collect();
    assert!(text.contains("→"));
    assert!(!text.contains("\\rightarrow"));
}

#[test]
fn inline_math_renders_leq_as_unicode() {
    let lines = project_markdown_to_lines("$x \\leq y$", None);
    let text: String = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.text.as_str())
        .collect();
    assert!(text.contains("≤"));
}

#[test]
fn display_math_renders_on_own_line() {
    let lines = project_markdown_to_lines("$$\\sum_{i=0}^{n} x_i$$", None);
    let text: String = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.text.as_str())
        .collect();
    assert!(text.contains("∑"));
}

#[test]
fn unknown_latex_passes_through() {
    let lines = project_markdown_to_lines("$\\foobar$", None);
    let text: String = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.text.as_str())
        .collect();
    assert!(text.contains("\\foobar"));
}

#[test]
fn plain_math_renders_as_text() {
    let lines = project_markdown_to_lines("$5$", None);
    let text: String = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.text.as_str())
        .collect();
    assert!(text.contains("5"));
    assert!(!text.contains("$"));
}

#[test]
fn inline_math_int_not_corrupted_by_in() {
    let lines = project_markdown_to_lines("$\\int_0^1 x \\, dx$", None);
    let text: String = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.text.as_str())
        .collect();
    assert!(
        text.contains("∫"),
        "integral should render as ∫, got: {text}"
    );
    assert!(!text.contains("∈t"), "int was corrupted by in prefix match");
}
