use super::*;

// ---------------------------------------------------------------------------
// Edit display title and section-label rows
// ---------------------------------------------------------------------------

#[test]
fn edit_preview_display_with_no_title_or_label_renders_only_diff() {
    let mut state = AppState::default();

    reduce_tool(&mut state, started("edit", r#"{"path":"sample.txt"}"#));

    reduce_tool(
        &mut state,
        ToolEvent::Completed {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"path":"sample.txt"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
                title: None,
                sections: vec![ToolDisplaySection {
                    label: None,
                    kind: ContentKind::Diff {
                        language: "diff".to_string(),
                        path: "test.rs".to_string(),
                    },
                    content: "--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n"
                        .to_string(),
                    stats: None,
                }],
            }),
            error_kind: None,
            message: None,
        },
    );

    let lines: Vec<String> = state
        .transcript
        .blocks()
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();
    assert!(!lines.contains(&"edit sample.txt".to_string()));
    assert!(
        !lines.contains(&"sample.txt (diff)".to_string()),
        "a None label must render no label row"
    );
    assert!(
        lines.iter().any(|line| line.contains("+++ b/sample.txt")),
        "the diff content must still render; got {lines:?}"
    );
}

/// The producer decides which rows render: a display with `title: None` and
/// `label: None` renders neither row, regardless of the tool name. The
/// renderer never inspects tool identity.
#[test]
fn completed_edit_display_with_no_title_or_label_renders_neither_row() -> Result<()> {
    let mut state = AppState::default();

    reduce_tool(
        &mut state,
        started("edit", r#"{\"path\":\"notes/todo.md\"}"#),
    );
    reduce_tool(
        &mut state,
        ToolEvent::Completed {
            name: "edit".to_string(),
            source: "mcp".to_string(),
            arguments: r#"{\"path\":\"notes/todo.md\"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
                title: None,
                sections: vec![ToolDisplaySection {
                    label: None,
                    kind: ContentKind::Diff {
                        language: "diff".to_string(),
                        path: "test.rs".to_string(),
                    },
                    content: "--- a\n+++ b\n@@ -1 +1 @@\n-old\n+new\n".to_string(),
                    stats: None,
                }],
            }),
            error_kind: None,
            message: None,
        },
    );

    let lines: Vec<String> = state
        .transcript
        .blocks()
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();
    assert!(
        !lines.iter().any(|line| line.contains("edit notes/todo.md")),
        "a None title must render no title row; got {lines:?}"
    );
    assert!(
        !lines
            .iter()
            .any(|line| line.contains("notes/todo.md (diff)")),
        "a None label must render no label row; got {lines:?}"
    );
    Ok(())
}

#[test]
fn edit_display_with_multiple_sections_keeps_section_labels() {
    let mut state = AppState::default();

    reduce_tool(&mut state, started("edit", r#"{"path":"sample.txt"}"#));

    reduce_tool(
        &mut state,
        ToolEvent::Completed {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"path":"sample.txt"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
                title: Some("edit sample.txt".to_string()),
                sections: vec![
                    ToolDisplaySection {
                        label: Some("sample.txt".to_string()),
                        kind: ContentKind::Diff {
                            language: "diff".to_string(),
                            path: "test.rs".to_string(),
                        },
                        content: "--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n"
                            .to_string(),
                        stats: None,
                    },
                    ToolDisplaySection {
                        label: Some("other.txt".to_string()),
                        kind: ContentKind::Diff {
                            language: "diff".to_string(),
                            path: "test.rs".to_string(),
                        },
                        content: "--- a/other.txt\n+++ b/other.txt\n@@ -1 +1 @@\n-a\n+b\n"
                            .to_string(),
                        stats: None,
                    },
                ],
            }),
            error_kind: None,
            message: None,
        },
    );

    let lines: Vec<String> = state
        .transcript
        .blocks()
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();
    assert!(
        lines.iter().any(|line| line.contains("sample.txt (diff)")),
        "multi-section displays must keep every section label; got {lines:?}"
    );
    assert!(
        lines.iter().any(|line| line.contains("other.txt (diff)")),
        "multi-section displays must keep every section label; got {lines:?}"
    );
}

#[test]
fn non_diff_single_section_display_keeps_section_label() {
    let mut state = AppState::default();

    reduce_tool(&mut state, started("nu", r#"{"command":"ls"}"#));

    reduce_tool(
        &mut state,
        ToolEvent::Completed {
            name: "nu".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"command":"ls"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
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
            error_kind: None,
            message: None,
        },
    );

    let lines: Vec<String> = state
        .transcript
        .blocks()
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();
    assert!(
        lines.iter().any(|line| line.contains("sample.rs (rust)")),
        "non-diff single-section displays must keep the section label; got {lines:?}"
    );
}

#[test]
fn edit_preview_display_renders_stats_line() {
    let mut state = AppState::default();

    reduce_tool(&mut state, started("edit", r#"{"path":"sample.txt"}"#));

    reduce_tool(
        &mut state,
        ToolEvent::Completed {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"path":"sample.txt"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
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
            error_kind: None,
            message: None,
        },
    );

    let lines = state
        .transcript
        .blocks()
        .iter()
        .map(|block| block.source.plain_text())
        .collect::<Vec<_>>();
    assert!(
        lines.iter().any(|line| line.contains("files=1")),
        "a Some stats record must render its stats row; got {lines:?}"
    );
}

/// The producer decides which rows render: a display with `title: Some(...)`
/// and `label: Some(...)` renders both rows, regardless of the tool name. The
/// renderer never inspects tool identity.
#[test]
fn display_with_title_and_label_renders_both_rows() {
    let mut state = AppState::default();

    reduce_tool(&mut state, started("nu", r#"{"command":"ls"}"#));

    reduce_tool(
        &mut state,
        ToolEvent::Completed {
            name: "nu".to_string(),
            source: "mcp".to_string(),
            arguments: r#"{"command":"ls"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
                title: Some("edit bar.rs".to_string()),
                sections: vec![ToolDisplaySection {
                    label: Some("changes".to_string()),
                    kind: ContentKind::Diff {
                        language: "diff".to_string(),
                        path: "test.rs".to_string(),
                    },
                    content: "+new content".to_string(),
                    stats: None,
                }],
            }),
            error_kind: None,
            message: None,
        },
    );

    let lines: Vec<String> = state
        .transcript
        .blocks()
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();
    assert!(
        lines.iter().any(|line| line.contains("edit bar.rs")),
        "non-edit tool identity must keep the display title; got {lines:?}"
    );
    assert!(
        lines.iter().any(|line| line.contains("changes (diff)")),
        "non-edit tool identity must keep the section label; got {lines:?}"
    );
}
