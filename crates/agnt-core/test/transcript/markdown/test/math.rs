use super::*;

/// Flatten the projected lines of `markdown` into one plain string.
fn projected_text(markdown: &str) -> String {
    project_markdown_to_lines(markdown, None)
        .iter()
        .flat_map(|line| line.spans.iter())
        .map(|span| span.text.as_str())
        .collect()
}

/// Inline math renders LaTeX commands as Unicode and passes unknown commands
/// through verbatim. Each row is one markdown/expected/forbidden triple.
#[test]
fn inline_math_renders_unicode_table() {
    // -- Setup & Fixtures
    let cases = [
        ("model $\\rightarrow$ model", "→", "\\rightarrow"),
        ("$x \\leq y$", "≤", "\\leq"),
        ("$5$", "5", "$"),
        ("$\\foobar$", "\\foobar", ""),
        ("$\\int_0^1 x \\, dx$", "∫", "∈t"),
        ("$\\text{Plugin Config}$", "Plugin Config", "\\text"),
    ];

    // -- Exec & Check
    for (markdown, expected, forbidden) in cases {
        let text = projected_text(markdown);
        assert!(
            text.contains(expected),
            "{markdown:?} should render {expected:?}, got {text:?}"
        );
        if !forbidden.is_empty() {
            assert!(
                !text.contains(forbidden),
                "{markdown:?} must not contain {forbidden:?}, got {text:?}"
            );
        }
    }
}

/// Display math renders on its own line with the Unicode operator.
#[test]
fn display_math_renders_on_own_line() {
    // -- Exec
    let text = projected_text("$$\\sum_{i=0}^{n} x_i$$");

    // -- Check
    assert!(
        text.contains("∑"),
        "display math should render ∑, got {text:?}"
    );
}
