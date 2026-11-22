use super::highlight::{
    HighlightRequest, SyntaxTokenChannel, highlight_source_tokens, resolve_syntax_hint,
};
use super::syntax_packdump::{DEFAULT_LANGUAGES, SyntaxSetSpec, default_syntax_set};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// The entries syntect 5.3's stock set cannot carry, so they render Plain.
const PLAIN_BY_DESIGN: [&str; 2] = ["ts", "toml"];

/// Every default-list entry resolves on the shipped asset, or is Plain by design.
#[test]
fn syntax_packdump_default_languages_coherence() -> Result<()> {
    // -- Setup & Fixtures
    let syntax_set = default_syntax_set();

    // -- Exec & Check
    for language in DEFAULT_LANGUAGES {
        let resolves = resolve_syntax_hint(&syntax_set, language).is_some();
        if PLAIN_BY_DESIGN.contains(language) {
            assert!(
                !resolves,
                "{language} must stay Plain by design (absent from syntect 5.3)"
            );
        } else {
            assert!(
                resolves,
                "{language} must resolve on the shipped packdump asset"
            );
        }
    }
    Ok(())
}

/// The plain-by-design entries render one all-plain span per line.
#[test]
fn syntax_packdump_plain_by_design_entries_render_plain() -> Result<()> {
    // -- Exec & Check
    for language in PLAIN_BY_DESIGN {
        let lines = highlight_source_tokens(HighlightRequest {
            language_hint: Some(language),
            source: "let x = 1;",
        });
        assert_eq!(lines.len(), 1, "{language} should render one line");
        assert!(
            lines[0]
                .iter()
                .all(|span| span.channel == SyntaxTokenChannel::Plain),
            "{language} should render plain token channels"
        );
    }
    Ok(())
}

/// The config value maps to the three spec forms.
#[test]
fn syntax_packdump_from_config_maps_values() -> Result<()> {
    // -- Setup & Fixtures
    let empty: Vec<String> = Vec::new();
    let all = vec!["all".to_string()];
    let all_with_extra = vec!["all".to_string(), "rust".to_string()];
    let list = vec!["python".to_string(), "ruby".to_string()];

    // -- Exec & Check
    assert_eq!(SyntaxSetSpec::from_config(None), SyntaxSetSpec::Default);
    assert_eq!(
        SyntaxSetSpec::from_config(Some(&empty)),
        SyntaxSetSpec::Default
    );
    assert_eq!(SyntaxSetSpec::from_config(Some(&all)), SyntaxSetSpec::All);
    assert_eq!(
        SyntaxSetSpec::from_config(Some(&all_with_extra)),
        SyntaxSetSpec::Languages(all_with_extra.clone()),
        "\"all\" means everything only as the single entry"
    );
    assert_eq!(
        SyntaxSetSpec::from_config(Some(&list)),
        SyntaxSetSpec::Languages(list.clone())
    );
    Ok(())
}
