use super::*;

/// Inline markdown styles map to their `Md*` hints, and plain text stays
/// Normal. Each row is one markdown/hint/expected-text triple.
#[test]
fn render_markdown_lines_inline_styles_table() -> Result<()> {
    // -- Setup & Fixtures
    let cases = [
        ("**bold**", IrStyleHint::MdBold, "bold"),
        ("*italic*", IrStyleHint::MdItalic, "italic"),
        ("a `code` b", IrStyleHint::MdInlineCode, "code"),
    ];

    // -- Exec & Check
    for (markdown, hint, expected_text) in cases {
        let lines = render_markdown_lines(markdown, None);
        let span = lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .find(|span| span.hint == hint)
            .ok_or_else(|| format!("{markdown:?} should produce a {hint:?} span"))?;
        assert!(
            span.text.contains(expected_text),
            "{markdown:?} span text should contain {expected_text:?}, got {:?}",
            span.text
        );
    }

    // -- Check: empty input renders no lines.
    assert!(
        render_markdown_lines("", None).is_empty(),
        "empty input should render no lines"
    );

    // -- Check: plain text renders one all-Normal line.
    let plain = render_markdown_lines("hello", None);
    assert_eq!(plain.len(), 1, "plain text should render one line");
    assert!(
        plain[0].spans.iter().all(|s| s.hint == IrStyleHint::Normal),
        "plain text spans must all be Normal"
    );
    let concat: String = plain[0].spans.iter().map(|s| s.text.as_str()).collect();
    assert!(
        concat.contains("hello"),
        "plain line should contain the source text"
    );
    Ok(())
}

/// Blank-line handling and fenced-code syntax hints.
#[test]
fn render_markdown_lines_blank_and_fence_table() -> Result<()> {
    // -- Setup & Fixtures
    let collapsed = render_markdown_lines("first\n\n\nlast", None);
    let trimmed = render_markdown_lines("\n\nhello\n\n", None);
    let fenced = render_markdown_lines("```rust\nfn x() {}\n```", None);

    // -- Check: consecutive blank lines collapse to one separator.
    assert_eq!(
        collapsed.len(),
        3,
        "consecutive blanks collapse to one separator"
    );
    assert!(!collapsed[0].spans.is_empty(), "first line carries text");
    assert!(
        collapsed[1].spans.is_empty(),
        "middle line is the blank separator"
    );
    assert!(!collapsed[2].spans.is_empty(), "last line carries text");

    // -- Check: leading and trailing blanks are trimmed.
    assert_eq!(trimmed.len(), 1, "leading/trailing blanks are trimmed");
    assert!(!trimmed[0].spans.is_empty());

    // -- Check: a fenced rust block produces syntax-hinted code spans.
    assert!(!fenced.is_empty(), "fenced block must produce lines");
    let has_code = fenced.iter().flat_map(|l| l.spans.iter()).any(|s| {
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
    let has_keyword = fenced
        .iter()
        .flat_map(|l| l.spans.iter())
        .any(|s| s.hint == IrStyleHint::MdCodeKeyword);
    assert!(
        has_keyword,
        "fenced rust block should contain MdCodeKeyword span"
    );
    Ok(())
}
