use super::*;

// ---------------------------------------------------------------------------
// Edit display title and section-label rows
// ---------------------------------------------------------------------------

/// The producer decides which rows render: a display with `title: None` and
/// `label: None` renders neither row, a display with both renders both rows,
/// regardless of the tool name. The renderer never inspects tool identity.
#[test]
fn edit_display_title_and_label_row_matrix() -> Result<()> {
    // -- Setup & Fixtures
    let diff = "--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n";

    // -- Exec & Check: no title, no label → only the diff renders.
    let mut none = AppState::default();
    reduce_ui(&mut none, started("edit", r#"{"path":"sample.txt"}"#));
    complete_with_display(
        &mut none,
        "edit",
        r#"{"path":"sample.txt"}"#,
        Some(ToolDisplay {
            title: None,
            sections: vec![diff_section(None, diff)],
        }),
    );
    let lines: Vec<String> = none
        .transcript
        .entries()
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();
    assert!(!lines.contains(&"edit sample.txt".to_string()));
    assert!(!lines.contains(&"sample.txt (diff)".to_string()));
    assert!(lines.iter().any(|line| line.contains("+++ b/sample.txt")));

    // -- Exec & Check: no title, no label, non-edit tool → neither row.
    let mut neither = AppState::default();
    reduce_ui(&mut neither, started("edit", r#"{"path":"notes/todo.md"}"#));
    complete_with_display(
        &mut neither,
        "edit",
        r#"{"path":"notes/todo.md"}"#,
        Some(ToolDisplay {
            title: None,
            sections: vec![diff_section(
                None,
                "--- a\n+++ b\n@@ -1 +1 @@\n-old\n+new\n",
            )],
        }),
    );
    let lines: Vec<String> = neither
        .transcript
        .entries()
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();
    assert!(!lines.iter().any(|line| line.contains("edit notes/todo.md")));
    assert!(
        !lines
            .iter()
            .any(|line| line.contains("notes/todo.md (diff)"))
    );

    // -- Exec & Check: title and label → both rows render.
    let mut both = AppState::default();
    reduce_ui(&mut both, started("nu", r#"{"command":"ls"}"#));
    complete_with_display(
        &mut both,
        "nu",
        r#"{"command":"ls"}"#,
        Some(ToolDisplay {
            title: Some("edit bar.rs".to_string()),
            sections: vec![diff_section(Some("changes"), "+new content")],
        }),
    );
    let lines: Vec<String> = both
        .transcript
        .entries()
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();
    assert!(lines.iter().any(|line| line.contains("edit bar.rs")));
    assert!(lines.iter().any(|line| line.contains("changes (diff)")));

    // -- Setup & Fixtures: two diff sections.
    let mut multi = AppState::default();
    reduce_ui(&mut multi, started("edit", r#"{"path":"sample.txt"}"#));
    complete_with_display(
        &mut multi,
        "edit",
        r#"{"path":"sample.txt"}"#,
        Some(ToolDisplay {
            title: Some("edit sample.txt".to_string()),
            sections: vec![
                diff_section(
                    Some("sample.txt"),
                    "--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n",
                ),
                diff_section(
                    Some("other.txt"),
                    "--- a/other.txt\n+++ b/other.txt\n@@ -1 +1 @@\n-a\n+b\n",
                ),
            ],
        }),
    );

    // -- Check
    let lines: Vec<String> = multi
        .transcript
        .entries()
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();
    assert!(lines.iter().any(|line| line.contains("sample.txt (diff)")));
    assert!(lines.iter().any(|line| line.contains("other.txt (diff)")));

    // -- Setup & Fixtures: non-diff single section.
    let mut code = AppState::default();
    reduce_ui(&mut code, started("nu", r#"{"command":"ls"}"#));
    complete_with_display(
        &mut code,
        "nu",
        r#"{"command":"ls"}"#,
        Some(ToolDisplay {
            title: Some("nu sample.rs".to_string()),
            sections: vec![ToolDisplaySection {
                label: Some("sample.rs".to_string()),
                kind: ContentKind::Code {
                    language: "rust".to_string(),
                },
                content: "fn main() {}\n".to_string(),
                stats: None,
            }],
        }),
    );

    // -- Check
    let lines: Vec<String> = code
        .transcript
        .entries()
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();
    assert!(lines.iter().any(|line| line.contains("sample.rs (rust)")));

    // -- Setup & Fixtures: stats row.
    let mut stats = AppState::default();
    reduce_ui(&mut stats, started("edit", r#"{"path":"sample.txt"}"#));
    complete_with_display(
        &mut stats,
        "edit",
        r#"{"path":"sample.txt"}"#,
        Some(ToolDisplay {
            title: None,
            sections: vec![ToolDisplaySection {
                label: None,
                kind: ContentKind::Diff {
                    language: "diff".to_string(),
                    path: "test.rs".to_string(),
                },
                content: "--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n"
                    .to_string(),
                stats: Some(agnt_core::protocol::event::ToolDisplayStats {
                    files_changed: Some(1),
                    insertions: Some(3),
                    deletions: Some(1),
                    diff_truncated: Some(false),
                    omitted_files: Some(0),
                    omitted_hunks: Some(0),
                }),
            }],
        }),
    );

    // -- Check
    let lines: Vec<String> = stats
        .transcript
        .entries()
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();
    assert!(lines.iter().any(|line| line.contains("files=1")));
    Ok(())
}
