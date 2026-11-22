use std::path::PathBuf;

use syntect::{dumps, parsing::SyntaxSet};

use super::highlight::{
    HighlightRequest, SyntaxTokenChannel, highlight_source_tokens, highlight_with_syntax_set,
};
use super::syntax_packdump::{
    DEFAULT_LANGUAGES, SyntaxSetSpec, build_syntax_set, curated_syntax_set, default_syntax_set,
    init,
};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Set to `1` to rewrite the checked-in asset from the stock-derived set.
const REGEN_ENV: &str = "NU_AGENT_RENDER_REGEN_SYNTAX_DUMP";

/// The checked-in asset's path, resolved from the crate root.
fn asset_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/assets/default.packdump")
}

/// A fixed small snippet for every language on the default list.
///
/// The table is exhaustive on purpose: a language added to the list without a
/// snippet fails here rather than silently skipping the span proof.
fn snippet(language: &str) -> Option<&'static str> {
    let snippet = match language {
        "rust" => "fn main() { let x = 1; }",
        "python" => "def f():\n    return 1",
        "go" => "func main() { x := 1 }",
        "js" => "function f() { return \"x\"; }",
        "ts" => "const x: number = 1;",
        "json" => "{\"a\": 1}",
        "yaml" => "a: 1 # note",
        "toml" => "a = 1",
        "bash" => "echo \"hi\"",
        "sql" => "SELECT 1 FROM t;",
        "c" => "int main(void) { return 0; }",
        "cpp" => "int main() { return 0; }",
        "ruby" => "def f; 1; end",
        "diff" => "--- a\n+++ b\n@@ -1 +1 @@\n-a\n+b",
        "nu" => "let x = \"hi\" # note",
        "nix" => "let name = \"x\"; in name",
        _ => return None,
    };
    Some(snippet)
}

/// The checked-in asset keeps the spans the stock-derived set produces.
///
/// A syntect upgrade that changes stock spans fails here until the asset is
/// regenerated: set `NU_AGENT_RENDER_REGEN_SYNTAX_DUMP=1` on this run and the
/// test rewrites `src/assets/default.packdump` before checking the fresh write
/// loads back to the same spans.
#[test]
fn packdump_asset_keeps_stock_spans() -> Result<()> {
    // -- Setup & Fixtures
    let fresh = curated_syntax_set();
    let regen = std::env::var(REGEN_ENV).is_ok_and(|value| value == "1");
    let path = asset_path();
    if regen {
        dumps::dump_to_uncompressed_file(&fresh, &path)?;
        log::debug!("rewrote the highlight syntax packdump at {path:?}");
    }
    let reference: SyntaxSet = if regen {
        dumps::from_uncompressed_dump_file(&path)?
    } else {
        default_syntax_set()
    };

    // -- Exec & Check
    for language in DEFAULT_LANGUAGES {
        let source = snippet(language)
            .ok_or_else(|| format!("{language} is on the default list but has no snippet"))?;
        let request = HighlightRequest {
            language_hint: Some(language),
            source,
        };
        let expected = highlight_with_syntax_set(request, &fresh);
        let actual = highlight_with_syntax_set(request, &reference);
        assert_eq!(
            actual, expected,
            "{language} spans drifted from the stock-derived set; regenerate the asset"
        );
    }
    Ok(())
}

/// The shipped asset produces the spans the stock-derived set produces.
///
/// This is the span proof: it compares live against the stock set, so a syntect
/// upgrade that changes stock spans fails here until the asset is regenerated.
#[test]
fn packdump_default_set_matches_the_stock_spans() -> Result<()> {
    // -- Setup & Fixtures
    let default_set = build_syntax_set(&SyntaxSetSpec::Default);
    let all_set = build_syntax_set(&SyntaxSetSpec::All);

    // -- Exec & Check
    for language in DEFAULT_LANGUAGES {
        let source = snippet(language)
            .ok_or_else(|| format!("{language} is on the default list but has no snippet"))?;
        let request = HighlightRequest {
            language_hint: Some(language),
            source,
        };
        assert_eq!(
            highlight_with_syntax_set(request, &all_set),
            highlight_with_syntax_set(request, &default_set),
            "{language} spans must match the stock-derived set"
        );
    }
    Ok(())
}

/// All mode resolves every syntax the stock set carries.
#[test]
fn packdump_all_mode_resolves_markdown() -> Result<()> {
    // -- Setup & Fixtures
    init(SyntaxSetSpec::All);
    let source = "# title\n\n- item\n";

    // -- Exec & Check
    for language in ["md", "markdown"] {
        let channels = highlight_source_tokens(HighlightRequest {
            language_hint: Some(language),
            source,
        })
        .iter()
        .flat_map(|line| line.iter().map(|span| span.channel))
        .collect::<Vec<_>>();
        assert!(
            channels
                .iter()
                .any(|channel| *channel != SyntaxTokenChannel::Plain),
            "{language} should resolve in all mode, got {channels:?}"
        );
    }
    Ok(())
}

/// Languages mode resolves the listed language and hides the rest.
#[test]
fn packdump_languages_mode_restricts_the_set() -> Result<()> {
    // -- Setup & Fixtures
    init(SyntaxSetSpec::Languages(vec!["python".to_string()]));

    // -- Exec
    let python = highlight_source_tokens(HighlightRequest {
        language_hint: Some("python"),
        source: "def f():\n    return 1",
    })
    .iter()
    .flat_map(|line| line.iter().map(|span| span.channel))
    .collect::<Vec<_>>();
    let ruby = highlight_source_tokens(HighlightRequest {
        language_hint: Some("ruby"),
        source: "def f; 1; end",
    });

    // -- Check
    assert!(
        python
            .iter()
            .any(|channel| *channel != SyntaxTokenChannel::Plain),
        "python should resolve in languages mode, got {python:?}"
    );
    assert_eq!(ruby.len(), 1, "ruby should render one line");
    assert_eq!(ruby[0].len(), 1, "ruby should render a single span");
    assert_eq!(
        ruby[0][0].channel,
        SyntaxTokenChannel::Plain,
        "ruby is not on the configured list, so it renders plain"
    );
    Ok(())
}

/// An entry the stock set cannot resolve is skipped, and the rest is unchanged.
#[test]
fn packdump_languages_mode_skips_an_unknown_entry() -> Result<()> {
    // -- Setup & Fixtures
    let with_unknown = build_syntax_set(&SyntaxSetSpec::Languages(vec![
        "python".to_string(),
        "not-a-real-language".to_string(),
    ]));
    let python_only = build_syntax_set(&SyntaxSetSpec::Languages(vec!["python".to_string()]));
    let request = HighlightRequest {
        language_hint: Some("python"),
        source: "def f():\n    return 1",
    };
    let skipped = HighlightRequest {
        language_hint: Some("not-a-real-language"),
        source: "def f():\n    return 1",
    };

    // -- Check
    assert_eq!(
        highlight_with_syntax_set(request, &with_unknown),
        highlight_with_syntax_set(request, &python_only),
        "the unknown entry must not change the resolvable language"
    );
    assert_eq!(
        highlight_with_syntax_set(skipped, &with_unknown),
        highlight_with_syntax_set(skipped, &python_only),
        "the unknown entry itself stays unresolved"
    );
    Ok(())
}
