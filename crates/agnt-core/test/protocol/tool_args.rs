//! Direct tests for the call-line fallback summary on [`CallLine`].

use super::tool_args::CallLine;

/// The fallback summary is the arrow plus the normalized arguments.
#[test]
fn from_json_summary_prefixes_the_arguments_with_the_arrow() {
    // -- Setup & Fixtures
    let table = [
        ("{}", "→ {}"),
        ("{\"path\":\"a.rs\"}", "→ {\"path\":\"a.rs\"}"),
        ("", "→ "),
    ];

    // -- Exec & Check
    for (arguments, expected) in table {
        let call = CallLine::from_json_summary(arguments);
        assert_eq!(call.summary, expected, "summary for {arguments:?}");
    }
}

/// The summary normalizes whitespace: runs of whitespace collapse and a comma
/// takes exactly one trailing space.
#[test]
fn from_json_summary_normalizes_whitespace_and_commas() {
    // -- Setup & Fixtures
    let table = [
        ("{\n  \"a\":   1\n}", "→ { \"a\": 1 }"),
        ("{\"a\":1,\"b\":2}", "→ {\"a\":1, \"b\":2}"),
        ("{\"a\":1,   \"b\":2}", "→ {\"a\":1, \"b\":2}"),
    ];

    // -- Exec & Check
    for (arguments, expected) in table {
        let call = CallLine::from_json_summary(arguments);
        assert_eq!(call.summary, expected, "summary for {arguments:?}");
    }
}

/// Over-long arguments truncate at 120 characters and gain an ellipsis; the
/// exact limit keeps every character and no ellipsis.
#[test]
fn from_json_summary_truncates_at_120_characters() {
    // -- Setup & Fixtures
    let at_limit = "x".repeat(120);
    let over_limit = "x".repeat(200);

    // -- Exec
    let at_limit_call = CallLine::from_json_summary(&at_limit);
    let over_limit_call = CallLine::from_json_summary(&over_limit);

    // -- Check
    assert_eq!(at_limit_call.summary, format!("→ {at_limit}"));
    assert_eq!(over_limit_call.summary, format!("→ {}…", "x".repeat(120)));
}

/// The transcript's plain-text form drops the arrow, so a caller that adds its
/// own arrow never renders two.
#[test]
fn from_json_summary_plain_text_drops_the_arrow() {
    // -- Setup & Fixtures
    let call = CallLine::from_json_summary("{\"path\":\"a.rs\"}");

    // -- Exec
    let plain = call.to_plain_text();

    // -- Check
    assert_eq!(plain, "{\"path\":\"a.rs\"}");
}
