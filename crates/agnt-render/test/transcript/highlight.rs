use super::highlight::{
    HighlightRequest, SyntaxTokenChannel, cached_syntax_set, highlight_source_tokens,
};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// The token channels produced for `source` under `language`.
fn token_channels(source: &str, language: &str) -> Vec<SyntaxTokenChannel> {
    highlight_source_tokens(HighlightRequest {
        language_hint: Some(language),
        source,
    })
    .iter()
    .flat_map(|line| line.iter().map(|span| span.channel))
    .collect()
}

/// The plain text of each highlighted line for `source` under `language`.
fn rendered_lines(source: &str, language: &str) -> Vec<String> {
    highlight_source_tokens(HighlightRequest {
        language_hint: Some(language),
        source,
    })
    .iter()
    .map(|line| {
        line.iter()
            .map(|span| span.text.as_str())
            .collect::<String>()
    })
    .collect()
}

/// Known languages produce non-plain tokens; unknown hints fall back to plain.
#[test]
fn highlight_source_tokens_language_table() -> Result<()> {
    // -- Setup & Fixtures: (language, source, channels that must be present)
    let cases = [
        ("rust", "fn main() {}", vec![]),
        (
            "javascript",
            "function brewStatus(orderName, shots = 2) {\n  // brew\n  return \"ok\";\n}",
            vec![
                SyntaxTokenChannel::Function,
                SyntaxTokenChannel::String,
                SyntaxTokenChannel::Comment,
            ],
        ),
        (
            "node",
            "function brewStatus() { return \"ok\"; }",
            vec![SyntaxTokenChannel::Function],
        ),
        (
            "nu",
            "let x = \"hello\" # comment",
            vec![
                SyntaxTokenChannel::Keyword,
                SyntaxTokenChannel::String,
                SyntaxTokenChannel::Comment,
            ],
        ),
        ("nushell", "let x = \"hello\" # comment", vec![]),
        (
            "nix",
            "let\n  name = \"nu-agent\";\n  enabled = true; # keep this\nin name",
            vec![SyntaxTokenChannel::Comment],
        ),
    ];

    // -- Exec & Check
    for (language, source, expected) in cases {
        if !source.contains('\n') {
            let rendered = rendered_lines(source, language);
            assert_eq!(
                rendered.join("\n"),
                source,
                "{language} must preserve the source text"
            );
        }
        let channels = token_channels(source, language);
        for channel in expected {
            assert!(
                channels.contains(&channel),
                "{language} should include {channel:?}, got {channels:?}"
            );
        }
        if matches!(language, "rust" | "nushell") {
            assert!(
                channels.iter().any(|c| *c != SyntaxTokenChannel::Plain),
                "{language} should include at least one non-plain token"
            );
        }
        if language == "nix" {
            let unique =
                channels
                    .iter()
                    .fold(Vec::new(), |mut acc: Vec<SyntaxTokenChannel>, ch| {
                        if !acc.contains(ch) {
                            acc.push(*ch);
                        }
                        acc
                    });
            assert!(
                unique.len() >= 3,
                "nix scope-based highlighting should produce semantic variation"
            );
        }
    }

    // -- Check: unknown language hints fall back to plain-only tokens.
    for language in ["not-a-real-language", "madeuplang"] {
        let lines = highlight_source_tokens(HighlightRequest {
            language_hint: Some(language),
            source: "fn main() {}",
        });
        assert_eq!(lines.len(), 1, "{language} should render one line");
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
            "{language} should render plain token channel"
        );
    }

    // -- Check: the nu pipeline yields Function or Operator channels.
    let pipeline = token_channels("ls | where size > 1mb", "nu");
    assert!(
        pipeline.contains(&SyntaxTokenChannel::Function)
            || pipeline.contains(&SyntaxTokenChannel::Operator),
        "expected nu pipeline to include Function or Operator channels, got: {pipeline:?}"
    );
    Ok(())
}

/// Empty input yields no lines; CRLF input splits into stable lines.
#[test]
fn highlight_source_tokens_empty_and_crlf_table() {
    // -- Exec & Check: empty input yields no lines.
    let empty = highlight_source_tokens(HighlightRequest {
        language_hint: Some("rust"),
        source: "",
    });
    assert!(
        empty.is_empty(),
        "empty input should produce empty line set"
    );

    // -- Check: CRLF input splits into stable lines without the carriage return.
    let crlf = highlight_source_tokens(HighlightRequest {
        language_hint: Some("rust"),
        source: "let x = 1;\r\nlet y = 2;",
    });
    let rendered = crlf
        .iter()
        .map(|line| {
            line.iter()
                .map(|span| span.text.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>();
    assert_eq!(rendered, vec!["let x = 1;", "let y = 2;"]);
    assert_eq!(crlf.len(), 2);
}

/// Malformed source does not panic and keeps its text readable.
#[test]
fn highlight_source_tokens_malformed_table() -> Result<()> {
    // -- Setup & Fixtures: (language, malformed source, fragments that survive)
    let cases = [
        (
            "rust",
            "fn main() {\n\u{0}\n\x07\n}",
            vec!["fn main() {", "}"],
        ),
        (
            "nu",
            "let s = \"unterminated\n\u{0}\nls | open file",
            vec!["\"unterminated", "ls | open file"],
        ),
    ];

    // -- Exec & Check
    for (language, source, fragments) in cases {
        let result = std::panic::catch_unwind(|| {
            highlight_source_tokens(HighlightRequest {
                language_hint: Some(language),
                source,
            })
        });
        assert!(
            result.is_ok(),
            "{language} highlighting should not panic on malformed source"
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
        for fragment in fragments {
            assert!(
                joined.contains(fragment),
                "{language} malformed output should contain {fragment:?}, got {joined:?}"
            );
        }
    }
    Ok(())
}

/// The syntax set is loaded once and cached.
#[test]
fn cached_syntax_set_returns_same_instance() {
    // -- Exec & Check
    let a = cached_syntax_set();
    let b = cached_syntax_set();
    assert!(
        std::ptr::eq(a, b),
        "cached_syntax_set must return the same instance"
    );
}
