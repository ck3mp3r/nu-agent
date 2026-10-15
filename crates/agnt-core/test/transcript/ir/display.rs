use super::*;

// ── DisplayStats::format_stats_line ─────────────────────────────────────────

/// Present counters format in fixed order, space-separated; `Some(false)`
/// truncation carries no value and must not render.
#[test]
fn display_stats_format_line_table() {
    // -- Setup & Fixtures
    let cases = [
        ("all none", DisplayStats::default(), None),
        (
            "present fields",
            DisplayStats {
                files_changed: Some(2),
                insertions: Some(5),
                deletions: Some(1),
                diff_truncated: None,
                omitted_files: None,
                omitted_hunks: None,
            },
            Some("files=2 +5 -1"),
        ),
        (
            "truncation and omissions",
            DisplayStats {
                files_changed: Some(3),
                insertions: Some(10),
                deletions: Some(4),
                diff_truncated: Some(true),
                omitted_files: Some(1),
                omitted_hunks: Some(2),
            },
            Some("files=3 +10 -4 truncated=true omitted=1 hunks_omitted=2"),
        ),
        (
            "false truncation",
            DisplayStats {
                diff_truncated: Some(false),
                ..DisplayStats::default()
            },
            None,
        ),
    ];

    // -- Exec & Check
    for (label, stats, expected) in cases {
        assert_eq!(
            stats.format_stats_line().as_deref(),
            expected,
            "{label} formatted wrong"
        );
    }
}

// ── Display / DisplaySection / ContentKind ──────────────────────────────────

/// `Display` stores its title and sections; `ContentKind::language` returns
/// the language for diff/code and an empty string for plain.
#[test]
fn display_and_content_kind_table() {
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

    // -- Exec & Check: Display stores title and sections.
    assert_eq!(display.title, Some("edit a.rs".to_string()));
    assert_eq!(display.sections.len(), 1);
    assert_eq!(display.sections[0].label, Some("a.rs".to_string()));
    assert_eq!(display.sections[0].content, "@@ -1 +1 @@");

    // -- Check: language() returns the language for diff/code, "" for plain.
    assert_eq!(
        ContentKind::Diff {
            language: "diff".to_string(),
            path: "test.rs".to_string(),
        }
        .language(),
        "diff"
    );
    assert_eq!(
        ContentKind::Code {
            language: "nu".to_string()
        }
        .language(),
        "nu"
    );
    assert_eq!(ContentKind::Plain.language(), "");

    // -- Check: ContentKind is comparable.
    assert_eq!(ContentKind::Plain, ContentKind::Plain);
    assert_ne!(
        ContentKind::Plain,
        ContentKind::Code {
            language: "nu".to_string()
        }
    );
}
