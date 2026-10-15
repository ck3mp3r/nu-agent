use super::*;

// ── Fill::from_preview ──────────────────────────────────────────────────────

/// A preview fills `Code` when any section carries diff or code content, and
/// `None` otherwise.
#[test]
fn fill_from_preview_table() {
    // -- Setup & Fixtures
    let plain = Display {
        title: Some("plain output".to_string()),
        sections: vec![DisplaySection {
            label: Some("out".to_string()),
            kind: ContentKind::Plain,
            content: "line".to_string(),
            stats: None,
        }],
    };
    let diff = Display {
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
    let code = Display {
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
    assert_eq!(
        Fill::from_preview(&None),
        Fill::None,
        "no preview fills none"
    );
    assert_eq!(
        Fill::from_preview(&Some(plain)),
        Fill::None,
        "plain-only preview fills none"
    );
    assert_eq!(
        Fill::from_preview(&Some(diff)),
        Fill::Code,
        "diff preview fills code"
    );
    assert_eq!(
        Fill::from_preview(&Some(code)),
        Fill::Code,
        "code preview fills code"
    );
}
