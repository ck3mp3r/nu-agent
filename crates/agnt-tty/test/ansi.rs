use agnt_core::transcript::ir::StyleHint;

use super::style_text;

#[test]
fn success_with_color_has_green_escape() {
    assert!(style_text("ok", &StyleHint::Success, true).contains("\x1b[32m"));
}

#[test]
fn no_color_returns_plain_text() {
    assert_eq!(style_text("plain", &StyleHint::Success, false), "plain");
}

#[test]
fn error_has_red_escape() {
    assert!(style_text("bad", &StyleHint::Error, true).contains("\x1b[31m"));
}

#[test]
fn muted_has_dim_escape() {
    assert!(style_text("dim", &StyleHint::Muted, true).contains("\x1b[2m"));
}

#[test]
fn normal_hint_has_no_escape() {
    assert_eq!(style_text("x", &StyleHint::Normal, true), "x");
}

// ========== markdown element hints (task 6b8be032) ==========

#[test]
fn md_heading_level_1_has_bold_red_escape() {
    assert_eq!(
        style_text("h", &StyleHint::MdHeading(1), true),
        "\x1b[1;31mh\x1b[0m"
    );
}

#[test]
fn md_heading_level_2_has_bold_yellow_escape() {
    assert_eq!(
        style_text("h", &StyleHint::MdHeading(2), true),
        "\x1b[1;33mh\x1b[0m"
    );
}

#[test]
fn md_heading_level_3_has_bold_yellow_escape() {
    assert_eq!(
        style_text("h", &StyleHint::MdHeading(3), true),
        "\x1b[1;33mh\x1b[0m"
    );
}

#[test]
fn md_heading_level_4_has_bold_green_escape() {
    assert_eq!(
        style_text("h", &StyleHint::MdHeading(4), true),
        "\x1b[1;32mh\x1b[0m"
    );
}

#[test]
fn md_heading_level_5_has_bold_cyan_escape() {
    assert_eq!(
        style_text("h", &StyleHint::MdHeading(5), true),
        "\x1b[1;36mh\x1b[0m"
    );
}

#[test]
fn md_heading_level_6_has_bold_bright_magenta_escape() {
    assert_eq!(
        style_text("h", &StyleHint::MdHeading(6), true),
        "\x1b[1;95mh\x1b[0m"
    );
}

#[test]
fn md_heading_level_0_has_bold_blue_escape() {
    assert_eq!(
        style_text("h", &StyleHint::MdHeading(0), true),
        "\x1b[1;34mh\x1b[0m"
    );
}

#[test]
fn md_heading_level_above_6_has_bold_blue_escape() {
    assert_eq!(
        style_text("h", &StyleHint::MdHeading(7), true),
        "\x1b[1;34mh\x1b[0m"
    );
}

#[test]
fn md_link_has_bright_magenta_escape() {
    assert_eq!(
        style_text("link", &StyleHint::MdLink, true),
        "\x1b[95mlink\x1b[0m"
    );
}

#[test]
fn md_link_url_has_italic_underline_blue_escape() {
    assert_eq!(
        style_text("url", &StyleHint::MdLinkUrl, true),
        "\x1b[3;4;34murl\x1b[0m"
    );
}

#[test]
fn md_quote_has_magenta_escape() {
    assert_eq!(
        style_text("quote", &StyleHint::MdQuote, true),
        "\x1b[35mquote\x1b[0m"
    );
}

#[test]
fn md_list_marker_has_cyan_escape() {
    assert_eq!(
        style_text("- ", &StyleHint::MdListMarker, true),
        "\x1b[36m- \x1b[0m"
    );
}

#[test]
fn new_markdown_hints_without_color_return_plain_text() {
    let hints = [
        StyleHint::MdHeading(0),
        StyleHint::MdHeading(1),
        StyleHint::MdHeading(6),
        StyleHint::MdLink,
        StyleHint::MdLinkUrl,
        StyleHint::MdQuote,
        StyleHint::MdListMarker,
    ];

    for hint in hints {
        assert_eq!(style_text("plain", &hint, false), "plain");
    }
}

// ========== recolored ANSI fallback (task 6866a2f2) ==========

/// WHEN `style_text` receives `MdBold`, THE function SHALL emit bold red; WHEN
/// it receives `DiffHunk`, THE function SHALL CONTINUE TO emit plain bold.
#[test]
fn md_bold_has_bold_red_escape_and_diff_hunk_stays_plain_bold() {
    assert_eq!(
        style_text("b", &StyleHint::MdBold, true),
        "\x1b[1;31mb\x1b[0m"
    );
    assert_eq!(
        style_text("h", &StyleHint::DiffHunk, true),
        "\x1b[1mh\x1b[0m"
    );
}

/// WHEN `style_text` receives `MdItalic` or `MdBoldItalic`, THE function SHALL
/// emit red italic or red bold italic respectively.
#[test]
fn md_italic_and_bold_italic_have_red_escapes() {
    assert_eq!(
        style_text("i", &StyleHint::MdItalic, true),
        "\x1b[3;31mi\x1b[0m"
    );
    assert_eq!(
        style_text("bi", &StyleHint::MdBoldItalic, true),
        "\x1b[1;3;31mbi\x1b[0m"
    );
}

/// WHEN `style_text` receives `MdInlineCode`, THE function SHALL emit green
/// with no dim.
#[test]
fn md_inline_code_has_green_escape_without_dim() {
    assert_eq!(
        style_text("c", &StyleHint::MdInlineCode, true),
        "\x1b[32mc\x1b[0m"
    );
}

/// WHEN `style_text` receives `MdCodeVariable`, THE function SHALL emit the
/// default foreground escape.
#[test]
fn md_code_variable_has_default_foreground_escape() {
    assert_eq!(
        style_text("v", &StyleHint::MdCodeVariable, true),
        "\x1b[39mv\x1b[0m"
    );
}

/// WHEN `style_text` receives `MdCodeConstant`, THE function SHALL emit yellow.
#[test]
fn md_code_constant_has_yellow_escape() {
    assert_eq!(
        style_text("k", &StyleHint::MdCodeConstant, true),
        "\x1b[33mk\x1b[0m"
    );
}

/// WHEN `style_text` receives `MdCodePunctuation`, THE function SHALL emit
/// bright black with no dim.
#[test]
fn md_code_punctuation_has_bright_black_escape_without_dim() {
    assert_eq!(
        style_text("p", &StyleHint::MdCodePunctuation, true),
        "\x1b[90mp\x1b[0m"
    );
}

/// WHEN `style_text` receives `MdCodeComment`, THE function SHALL emit italic
/// bright black with no dim.
#[test]
fn md_code_comment_has_italic_bright_black_escape_without_dim() {
    assert_eq!(
        style_text("// c", &StyleHint::MdCodeComment, true),
        "\x1b[3;90m// c\x1b[0m"
    );
}

/// WHEN `style_text` receives `MdCodeKeyword`, `MdCodeType`,
/// `MdCodeFunction`, `MdCodeString`, `MdCodeNumber`, or `MdCodeOperator`, THE
/// function SHALL CONTINUE TO emit the existing sequences.
#[test]
fn unchanged_code_hints_keep_existing_escapes() {
    let cases = [
        (StyleHint::MdCodeKeyword, "\x1b[35m"),
        (StyleHint::MdCodeType, "\x1b[33m"),
        (StyleHint::MdCodeFunction, "\x1b[34m"),
        (StyleHint::MdCodeString, "\x1b[32m"),
        (StyleHint::MdCodeNumber, "\x1b[33m"),
        (StyleHint::MdCodeOperator, "\x1b[36m"),
    ];

    for (hint, expected) in cases {
        assert_eq!(
            style_text("t", &hint, true),
            format!("{expected}t\x1b[0m"),
            "{hint:?} should keep its existing escape"
        );
    }
}
