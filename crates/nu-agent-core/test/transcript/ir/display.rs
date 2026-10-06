use super::*;

// ── Display / DisplaySection / ContentKind ──────────────────────────────────

#[test]
fn display_stores_title_and_sections() {
    // -- Setup & Fixtures
    let display = Display {
        title: "edit a.rs".to_string(),
        sections: vec![DisplaySection {
            label: "a.rs".to_string(),
            kind: ContentKind::Diff {
                language: "diff".to_string(),
            },
            content: "@@ -1 +1 @@".to_string(),
            stats: None,
        }],
    };

    // -- Check
    assert_eq!(display.title, "edit a.rs");
    assert_eq!(display.sections.len(), 1);
    assert_eq!(display.sections[0].label, "a.rs");
    assert_eq!(display.sections[0].content, "@@ -1 +1 @@");
}

/// `is_single_diff_section` is a pure shape query: exactly one section and it
/// is a diff. One code section, one plain section, or two sections are false.
#[test]
fn display_is_single_diff_section_is_a_pure_shape_query() {
    // -- Setup & Fixtures
    let one_diff = Display {
        title: "t".to_string(),
        sections: vec![DisplaySection {
            label: "l".to_string(),
            kind: ContentKind::Diff {
                language: "diff".to_string(),
            },
            content: String::new(),
            stats: None,
        }],
    };
    let one_code = Display {
        title: "t".to_string(),
        sections: vec![DisplaySection {
            label: "l".to_string(),
            kind: ContentKind::Code {
                language: "nu".to_string(),
            },
            content: String::new(),
            stats: None,
        }],
    };
    let one_plain = Display {
        title: "t".to_string(),
        sections: vec![DisplaySection {
            label: "l".to_string(),
            kind: ContentKind::Plain,
            content: String::new(),
            stats: None,
        }],
    };
    let two_diffs = Display {
        title: "t".to_string(),
        sections: vec![
            DisplaySection {
                label: "a".to_string(),
                kind: ContentKind::Diff {
                    language: "diff".to_string(),
                },
                content: String::new(),
                stats: None,
            },
            DisplaySection {
                label: "b".to_string(),
                kind: ContentKind::Diff {
                    language: "diff".to_string(),
                },
                content: String::new(),
                stats: None,
            },
        ],
    };
    let no_sections = Display {
        title: "t".to_string(),
        sections: vec![],
    };

    // -- Exec & Check
    assert!(one_diff.is_single_diff_section());
    assert!(!one_code.is_single_diff_section());
    assert!(!one_plain.is_single_diff_section());
    assert!(!two_diffs.is_single_diff_section());
    assert!(!no_sections.is_single_diff_section());
}

/// `is_single_code_section` is the sibling pure shape query: exactly one
/// section and it is code. One diff section, one plain section, or two
/// sections are false.
#[test]
fn display_is_single_code_section_is_a_pure_shape_query() {
    // -- Setup & Fixtures
    let one_code = Display {
        title: "t".to_string(),
        sections: vec![DisplaySection {
            label: "l".to_string(),
            kind: ContentKind::Code {
                language: "nu".to_string(),
            },
            content: String::new(),
            stats: None,
        }],
    };
    let one_diff = Display {
        title: "t".to_string(),
        sections: vec![DisplaySection {
            label: "l".to_string(),
            kind: ContentKind::Diff {
                language: "diff".to_string(),
            },
            content: String::new(),
            stats: None,
        }],
    };
    let one_plain = Display {
        title: "t".to_string(),
        sections: vec![DisplaySection {
            label: "l".to_string(),
            kind: ContentKind::Plain,
            content: String::new(),
            stats: None,
        }],
    };
    let two_codes = Display {
        title: "t".to_string(),
        sections: vec![
            DisplaySection {
                label: "a".to_string(),
                kind: ContentKind::Code {
                    language: "nu".to_string(),
                },
                content: String::new(),
                stats: None,
            },
            DisplaySection {
                label: "b".to_string(),
                kind: ContentKind::Code {
                    language: "nu".to_string(),
                },
                content: String::new(),
                stats: None,
            },
        ],
    };
    let no_sections = Display {
        title: "t".to_string(),
        sections: vec![],
    };

    // -- Exec & Check
    assert!(one_code.is_single_code_section());
    assert!(!one_diff.is_single_code_section());
    assert!(!one_plain.is_single_code_section());
    assert!(!two_codes.is_single_code_section());
    assert!(!no_sections.is_single_code_section());
}

#[test]
fn content_kind_language_returns_language_for_diff_and_code() {
    // -- Exec & Check
    assert_eq!(
        ContentKind::Diff {
            language: "diff".to_string()
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
}

#[test]
fn content_kind_language_returns_empty_for_plain() {
    assert_eq!(ContentKind::Plain.language(), "");
}

#[test]
fn content_kind_is_comparable() {
    // -- Setup & Fixtures
    let kind = ContentKind::Plain;

    // -- Exec & Check
    assert_eq!(kind, ContentKind::Plain);
    assert_ne!(
        kind,
        ContentKind::Code {
            language: "nu".to_string()
        }
    );
}

// ── DisplayStats::format_stats_line ──────────────────────────────────────

/// A stats record with every field `None` formats to no line.
#[test]
fn stats_format_line_all_none_returns_none() {
    // -- Setup & Fixtures
    let stats = DisplayStats::default();

    // -- Exec & Check
    assert!(
        stats.format_stats_line().is_none(),
        "empty stats must format to None"
    );
}

/// Present counters format in fixed order, space-separated.
#[test]
fn stats_format_line_formats_present_fields() -> Result<()> {
    // -- Setup & Fixtures
    let stats = DisplayStats {
        files_changed: Some(2),
        insertions: Some(5),
        deletions: Some(1),
        diff_truncated: None,
        omitted_files: None,
        omitted_hunks: None,
    };

    // -- Exec
    let line = stats.format_stats_line();

    // -- Check
    assert_eq!(line.as_deref(), Some("files=2 +5 -1"));
    Ok(())
}

/// Truncation renders only when `Some(true)`; omissions render their counts.
#[test]
fn stats_format_line_renders_truncation_and_omissions() -> Result<()> {
    // -- Setup & Fixtures
    let stats = DisplayStats {
        files_changed: Some(3),
        insertions: Some(10),
        deletions: Some(4),
        diff_truncated: Some(true),
        omitted_files: Some(1),
        omitted_hunks: Some(2),
    };

    // -- Exec
    let line = stats.format_stats_line();

    // -- Check
    assert_eq!(
        line.as_deref(),
        Some("files=3 +10 -4 truncated=true omitted=1 hunks_omitted=2")
    );
    Ok(())
}

/// `Some(false)` truncation carries no value and must not render.
#[test]
fn stats_format_line_false_truncation_is_omitted() {
    // -- Setup & Fixtures: only a `false` flag — no renderable value.
    let stats = DisplayStats {
        diff_truncated: Some(false),
        ..DisplayStats::default()
    };

    // -- Exec & Check
    assert!(
        stats.format_stats_line().is_none(),
        "Some(false) truncation is not a value; got {:?}",
        stats.format_stats_line()
    );
}
