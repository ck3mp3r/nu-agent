use super::highlight::{
    HighlightRequest, SyntaxTokenChannel, cached_syntax_set, highlight_source_tokens,
};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn highlights_known_rust_language_with_keyword_style() {
    let lines = highlight_source_tokens(HighlightRequest {
        language_hint: Some("rust"),
        source: "fn main() {}",
    });

    let rendered = lines
        .iter()
        .map(|line| {
            line.iter()
                .map(|span| span.text.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>();
    assert_eq!(rendered, vec!["fn main() {}"]);

    let has_highlighted_channel = lines[0]
        .iter()
        .any(|span| span.channel != SyntaxTokenChannel::Plain);
    assert!(
        has_highlighted_channel,
        "known rust highlight should include at least one non-plain token"
    );
}

#[test]
fn falls_back_to_plain_style_for_unknown_language_hint() {
    let lines = highlight_source_tokens(HighlightRequest {
        language_hint: Some("not-a-real-language"),
        source: "fn main() {}",
    });

    assert_eq!(lines.len(), 1);
    assert_eq!(
        lines[0]
            .iter()
            .map(|span| span.text.as_str())
            .collect::<String>(),
        "fn main() {}"
    );
    assert!(
        lines[0]
            .iter()
            .all(|span| span.channel == SyntaxTokenChannel::Plain),
        "unknown language should render plain token channel"
    );
}

#[test]
fn empty_input_returns_deterministic_empty_contract() {
    let lines = highlight_source_tokens(HighlightRequest {
        language_hint: Some("rust"),
        source: "",
    });

    assert!(
        lines.is_empty(),
        "empty input should produce empty line set"
    );
}

#[test]
fn malformed_source_does_not_panic_and_remains_readable() -> Result<()> {
    let malformed = "fn main() {\n\u{0}\n\x07\n}";

    let result = std::panic::catch_unwind(|| {
        highlight_source_tokens(HighlightRequest {
            language_hint: Some("rust"),
            source: malformed,
        })
    });

    assert!(
        result.is_ok(),
        "highlighting should not panic on malformed source"
    );
    let lines = result.map_err(|_| "catch_unwind should return highlighted lines")?;
    let joined = lines
        .iter()
        .map(|line| {
            line.iter()
                .map(|span| span.text.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(joined.contains("fn main() {"));
    assert!(joined.contains("}"));
    Ok(())
}

#[test]
fn line_splitting_is_stable_for_crlf_input() {
    let lines = highlight_source_tokens(HighlightRequest {
        language_hint: Some("rust"),
        source: "let x = 1;\r\nlet y = 2;",
    });

    let rendered = lines
        .iter()
        .map(|line| {
            line.iter()
                .map(|span| span.text.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>();
    assert_eq!(rendered, vec!["let x = 1;", "let y = 2;"]);
    assert_eq!(lines.len(), 2);
}

#[test]
fn javascript_scope_semantics_include_function_string_and_comment_channels() {
    let lines = highlight_source_tokens(HighlightRequest {
        language_hint: Some("javascript"),
        source: "function brewStatus(orderName, shots = 2) {\n  // brew
  return \"ok\";\n}",
    });

    let channels = lines
        .iter()
        .flat_map(|line| line.iter().map(|span| span.channel))
        .collect::<Vec<_>>();

    assert!(channels.contains(&SyntaxTokenChannel::Function));
    assert!(
        channels.contains(&SyntaxTokenChannel::String),
        "expected javascript channels to include String, got: {:?}",
        channels
    );
    assert!(channels.contains(&SyntaxTokenChannel::Comment));
}

#[test]
fn nix_scope_semantics_include_keyword_comment_and_variable_channels() {
    let lines = highlight_source_tokens(HighlightRequest {
        language_hint: Some("nix"),
        source: "let
  name = \"nu-agent\";
  enabled = true; # keep this
in name",
    });

    let channels = lines
        .iter()
        .flat_map(|line| line.iter().map(|span| span.channel))
        .collect::<Vec<_>>();

    assert!(channels.contains(&SyntaxTokenChannel::Comment));

    let unique = channels.into_iter().fold(Vec::new(), |mut acc, ch| {
        if !acc.contains(&ch) {
            acc.push(ch);
        }
        acc
    });
    assert!(
        unique.len() >= 3,
        "nix scope-based highlighting should produce semantic variation"
    );
}

#[test]
fn normalizes_javascript_alias_to_js_syntax() {
    let lines = highlight_source_tokens(HighlightRequest {
        language_hint: Some("javascript"),
        source: "function brewStatus() { return \"ok\"; }",
    });

    let channels = lines
        .iter()
        .flat_map(|line| line.iter().map(|span| span.channel))
        .collect::<Vec<_>>();

    assert!(channels.contains(&SyntaxTokenChannel::Function));
}

#[test]
fn normalizes_node_alias_to_js_syntax() {
    let lines = highlight_source_tokens(HighlightRequest {
        language_hint: Some("node"),
        source: "function brewStatus() { return \"ok\"; }",
    });

    let channels = lines
        .iter()
        .flat_map(|line| line.iter().map(|span| span.channel))
        .collect::<Vec<_>>();

    assert!(channels.contains(&SyntaxTokenChannel::Function));
}

#[test]
fn unsupported_language_fallback_is_plain_only() {
    let lines = highlight_source_tokens(HighlightRequest {
        language_hint: Some("madeuplang"),
        source: "function brewStatus() { return \"ok\"; }",
    });

    assert!(
        lines
            .iter()
            .flat_map(|line| line.iter())
            .all(|span| span.channel == SyntaxTokenChannel::Plain)
    );
}

#[test]
fn cached_syntax_set_returns_same_instance() {
    let a = cached_syntax_set();
    let b = cached_syntax_set();
    assert!(std::ptr::eq(a, b));
}

#[test]
fn nu_hint_highlights_keyword_string_and_comment_channels() {
    let lines = highlight_source_tokens(HighlightRequest {
        language_hint: Some("nu"),
        source: "let x = \"hello\" # comment",
    });

    let rendered = lines
        .iter()
        .map(|line| {
            line.iter()
                .map(|span| span.text.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>();
    assert_eq!(rendered, vec!["let x = \"hello\" # comment"]);

    let channels = lines
        .iter()
        .flat_map(|line| line.iter().map(|span| span.channel))
        .collect::<Vec<_>>();

    assert!(
        channels.contains(&SyntaxTokenChannel::Keyword),
        "expected nu channels to include Keyword for `let`, got: {:?}",
        channels
    );
    assert!(
        channels.contains(&SyntaxTokenChannel::String),
        "expected nu channels to include String for the quoted literal, got: {:?}",
        channels
    );
    assert!(
        channels.contains(&SyntaxTokenChannel::Comment),
        "expected nu channels to include Comment, got: {:?}",
        channels
    );
}

#[test]
fn nushell_alias_resolves_to_nu_syntax() {
    let lines = highlight_source_tokens(HighlightRequest {
        language_hint: Some("nushell"),
        source: "let x = \"hello\" # comment",
    });

    let has_highlighted_channel = lines
        .iter()
        .flat_map(|line| line.iter())
        .any(|span| span.channel != SyntaxTokenChannel::Plain);
    assert!(
        has_highlighted_channel,
        "nushell alias should resolve to the nu syntax definition"
    );
}

#[test]
fn nu_pipeline_and_builtins_highlight_as_function_or_operator() {
    let lines = highlight_source_tokens(HighlightRequest {
        language_hint: Some("nu"),
        source: "ls | where size > 1mb",
    });

    let channels = lines
        .iter()
        .flat_map(|line| line.iter().map(|span| span.channel))
        .collect::<Vec<_>>();

    assert!(
        channels.contains(&SyntaxTokenChannel::Function)
            || channels.contains(&SyntaxTokenChannel::Operator),
        "expected nu pipeline to include Function or Operator channels, got: {:?}",
        channels
    );
}

#[test]
fn nu_malformed_source_does_not_panic_and_preserves_text() -> Result<()> {
    let malformed = "let s = \"unterminated\n\u{0}\nls | open file";

    let result = std::panic::catch_unwind(|| {
        highlight_source_tokens(HighlightRequest {
            language_hint: Some("nu"),
            source: malformed,
        })
    });

    assert!(
        result.is_ok(),
        "highlighting should not panic on malformed nu source"
    );
    let lines = match result {
        Ok(lines) => lines,
        Err(_) => return Err("catch_unwind should return highlighted lines".into()),
    };
    let joined = lines
        .iter()
        .map(|line| {
            line.iter()
                .map(|span| span.text.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(joined.contains("\"unterminated"));
    assert!(joined.contains("ls | open file"));
    Ok(())
}
