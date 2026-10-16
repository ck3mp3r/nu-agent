use super::*;

// ========== markdown element hints (task 6b0f16da) ==========

/// WHEN the projector projects an ATX heading of level N (1-6), THE projector
/// SHALL emit the heading text spans with `StyleHint::MdHeading(N)`.
#[test]
fn markdown_projection_headings_emit_level_hints() -> Result<()> {
    // -- Setup & Fixtures
    let markdown = "# H1\n\n## H2\n\n### H3\n\n#### H4\n\n##### H5\n\n###### H6\n";

    // -- Exec
    let lines = project_markdown_to_lines(markdown, None);

    // -- Check
    for level in 1..=6u8 {
        let needle = format!("H{level}");
        let line = lines
            .iter()
            .find(|l| plain_line(l) == needle)
            .ok_or_else(|| format!("heading line {needle:?} should be projected"))?;
        assert!(
            line.spans
                .iter()
                .any(|s| s.hint == StyleHint::MdHeading(level)),
            "heading {needle:?} should carry MdHeading({level}), got {:?}",
            line.spans
                .iter()
                .map(|s| s.hint.clone())
                .collect::<Vec<_>>()
        );
    }
    Ok(())
}

/// WHEN the projector projects a list item marker, THE projector SHALL emit
/// the marker span with `StyleHint::MdListMarker`.
#[test]
fn markdown_projection_list_marker_emits_md_list_marker() -> Result<()> {
    // -- Setup & Fixtures
    let markdown = "- one\n- two\n\n1. first\n";

    // -- Exec
    let lines = project_markdown_to_lines(markdown, None);

    // -- Check: the bullet marker carries MdListMarker.
    let bullet = lines
        .iter()
        .find(|l| plain_line(l) == "• one")
        .ok_or("bullet item should be projected")?;
    assert!(
        bullet
            .spans
            .iter()
            .any(|s| s.text == "• " && s.hint == StyleHint::MdListMarker),
        "bullet marker should carry MdListMarker, got {:?}",
        bullet
            .spans
            .iter()
            .map(|s| (s.text.clone(), s.hint.clone()))
            .collect::<Vec<_>>()
    );

    // -- Check: the ordered marker carries MdListMarker.
    let ordered = lines
        .iter()
        .find(|l| plain_line(l) == "1. first")
        .ok_or("ordered item should be projected")?;
    assert!(
        ordered
            .spans
            .iter()
            .any(|s| s.text == "1. " && s.hint == StyleHint::MdListMarker),
        "ordered marker should carry MdListMarker"
    );
    Ok(())
}

/// WHEN the projector projects block quote text or a block quote prefix, THE
/// projector SHALL emit those spans with `StyleHint::MdQuote`.
#[test]
fn markdown_projection_blockquote_emits_md_quote() -> Result<()> {
    // -- Setup & Fixtures
    let markdown = "> quoted\n";

    // -- Exec
    let lines = project_markdown_to_lines(markdown, None);

    // -- Check
    let line = lines
        .iter()
        .find(|l| plain_line(l) == "│ quoted")
        .ok_or("blockquote line should be projected")?;
    assert!(
        line.spans
            .iter()
            .any(|s| s.text == "│ " && s.hint == StyleHint::MdQuote),
        "blockquote prefix should carry MdQuote, got {:?}",
        line.spans
            .iter()
            .map(|s| (s.text.clone(), s.hint.clone()))
            .collect::<Vec<_>>()
    );
    assert!(
        line.spans
            .iter()
            .any(|s| s.text == "quoted" && s.hint == StyleHint::MdQuote),
        "blockquote text should carry MdQuote"
    );
    Ok(())
}

/// WHEN the projector projects a link label, THE projector SHALL emit the
/// label spans with `StyleHint::MdLink`; WHEN it reaches `TagEnd::Link`, THE
/// projector SHALL emit the destination suffix with `StyleHint::MdLinkUrl`.
#[test]
fn markdown_projection_link_emits_md_link_and_url() -> Result<()> {
    // -- Setup & Fixtures
    let markdown = "[label](https://example.com)\n";

    // -- Exec
    let lines = project_markdown_to_lines(markdown, None);

    // -- Check
    let line = lines
        .iter()
        .find(|l| plain_line(l).contains("label"))
        .ok_or("link line should be projected")?;
    assert!(
        line.spans
            .iter()
            .any(|s| s.text == "label" && s.hint == StyleHint::MdLink),
        "link label should carry MdLink, got {:?}",
        line.spans
            .iter()
            .map(|s| (s.text.clone(), s.hint.clone()))
            .collect::<Vec<_>>()
    );
    assert!(
        line.spans
            .iter()
            .any(|s| s.text == " (https://example.com)" && s.hint == StyleHint::MdLinkUrl),
        "link destination suffix should carry MdLinkUrl"
    );
    Ok(())
}

/// WHEN the projector projects a table header row, THE projector SHALL emit
/// the header cell spans with `StyleHint::MdHeading(0)`.
#[test]
fn markdown_projection_table_header_emits_md_heading_zero() -> Result<()> {
    // -- Setup & Fixtures
    let markdown = "| Name | Type |\n| --- | --- |\n| a | b |\n";

    // -- Exec
    let lines = project_markdown_to_lines(markdown, None);

    // -- Check
    let header = lines
        .iter()
        .find(|l| plain_line(l).contains("Name") && plain_line(l).contains("Type"))
        .ok_or("header row should be projected")?;
    assert!(
        header
            .spans
            .iter()
            .any(|s| s.hint == StyleHint::MdHeading(0)),
        "header cells should carry MdHeading(0), got {:?}",
        header
            .spans
            .iter()
            .map(|s| s.hint.clone())
            .collect::<Vec<_>>()
    );
    Ok(())
}

/// WHEN text inside a block quote or link carries emphasis, strong, or inline
/// code, THE projector SHALL CONTINUE TO emit MdItalic, MdBold, or
/// MdInlineCode for that text.
#[test]
fn markdown_projection_emphasis_inside_quote_and_link_keeps_own_hint() -> Result<()> {
    // -- Setup & Fixtures
    let markdown = "> *em* and **strong** and `code`\n\n[*em*](https://example.com)\n";

    // -- Exec
    let lines = project_markdown_to_lines(markdown, None);

    // -- Check: emphasis, strong, and inline code keep their own hints inside a
    // block quote.
    let quote = lines
        .iter()
        .find(|l| plain_line(l).contains("strong"))
        .ok_or("quote line should be projected")?;
    assert!(
        quote
            .spans
            .iter()
            .any(|s| s.text == "em" && s.hint == StyleHint::MdItalic),
        "emphasis inside a quote should stay MdItalic"
    );
    assert!(
        quote
            .spans
            .iter()
            .any(|s| s.text == "strong" && s.hint == StyleHint::MdBold),
        "strong inside a quote should stay MdBold"
    );
    assert!(
        quote
            .spans
            .iter()
            .any(|s| s.text == "code" && s.hint == StyleHint::MdInlineCode),
        "inline code inside a quote should stay MdInlineCode"
    );

    // -- Check: emphasis inside a link keeps MdItalic.
    let link = lines
        .iter()
        .find(|l| plain_line(l).contains("example.com"))
        .ok_or("link line should be projected")?;
    assert!(
        link.spans
            .iter()
            .any(|s| s.text == "em" && s.hint == StyleHint::MdItalic),
        "emphasis inside a link should stay MdItalic"
    );
    Ok(())
}

/// WHEN the projector projects paragraphs, fenced code blocks, or images, THE
/// projector SHALL CONTINUE TO emit the existing hints (Normal, MdCode* for
/// code, Normal for image suffixes).
#[test]
fn markdown_projection_paragraph_code_image_keep_existing_hints() -> Result<()> {
    // -- Setup & Fixtures
    let markdown =
        "plain paragraph\n\n![alt](https://img.example/x.png)\n\n```rust\nfn x() {}\n```\n";

    // -- Exec
    let lines = project_markdown_to_lines(markdown, None);

    // -- Check: paragraph text stays Normal.
    let para = lines
        .iter()
        .find(|l| plain_line(l) == "plain paragraph")
        .ok_or("paragraph should be projected")?;
    assert!(
        para.spans.iter().all(|s| s.hint == StyleHint::Normal),
        "paragraph text should stay Normal"
    );

    // -- Check: the image suffix stays Normal.
    let image = lines
        .iter()
        .find(|l| plain_line(l).contains("image:"))
        .ok_or("image line should be projected")?;
    assert!(
        image.spans.iter().all(|s| s.hint == StyleHint::Normal),
        "image suffix should stay Normal"
    );

    // -- Check: fenced code keeps its MdCode* hints.
    let code = lines
        .iter()
        .find(|l| plain_line(l).contains("fn x()"))
        .ok_or("code line should be projected")?;
    assert!(
        code.spans
            .iter()
            .any(|s| matches!(s.hint, StyleHint::MdCodeKeyword | StyleHint::MdCodePlain)),
        "fenced code should keep MdCode* hints"
    );
    Ok(())
}
