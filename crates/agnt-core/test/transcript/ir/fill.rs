use super::*;

// ── Fill::from_preview ──────────────────────────────────────────────────────

#[test]
fn fill_from_preview_none_returns_none() {
    // -- Exec & Check
    assert_eq!(Fill::from_preview(&None), Fill::None);
}

#[test]
fn fill_from_preview_plain_only_display_returns_none() -> Result<()> {
    // -- Setup & Fixtures
    let display = Display {
        title: Some("plain output".to_string()),
        sections: vec![DisplaySection {
            label: Some("out".to_string()),
            kind: ContentKind::Plain,
            content: "line".to_string(),
            stats: None,
        }],
    };

    // -- Exec & Check
    assert_eq!(Fill::from_preview(&Some(display)), Fill::None);
    Ok(())
}

#[test]
fn fill_from_preview_diff_section_returns_code() -> Result<()> {
    // -- Setup & Fixtures
    let display = Display {
        title: Some("edit a.rs".to_string()),
        sections: vec![DisplaySection {
            label: Some("a.rs".to_string()),
            kind: ContentKind::Diff {
                language: "diff".to_string(),
                path: "test.rs".to_string(),
            },
            content: "@@ -1 +1 @@".to_string(),
            stats: None,
        }],
    };

    // -- Exec & Check
    assert_eq!(Fill::from_preview(&Some(display)), Fill::Code);
    Ok(())
}

#[test]
fn fill_from_preview_code_section_returns_code() -> Result<()> {
    // -- Setup & Fixtures
    let display = Display {
        title: Some("run nu".to_string()),
        sections: vec![DisplaySection {
            label: Some("script".to_string()),
            kind: ContentKind::Code {
                language: "nu".to_string(),
            },
            content: "ls".to_string(),
            stats: None,
        }],
    };

    // -- Exec & Check
    assert_eq!(Fill::from_preview(&Some(display)), Fill::Code);
    Ok(())
}
