use super::*;

// ---------------------------------------------------------------------------
// Edit display title and section-label suppression
// ---------------------------------------------------------------------------

#[test]
fn edit_preview_display_omits_redundant_edit_path_header() {
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
                title: "edit sample.txt".to_string(),
                sections: vec![ToolDisplaySection {
                    label: "sample.txt".to_string(),
                    kind: ContentKind::Diff {
                        language: "diff".to_string(),
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
        "the call line already shows the path, so the section label is redundant"
    );
}

/// The completed-edit display suppression decision must be typed: the tool
/// name arrives on the event as `"edit"` and maps to `BuiltinKind::Edit` —
/// never a title-text prefix probe, which any title (including a user-visible
/// path like `edit notes/todo.md`) can false-match.
#[test]
fn completed_edit_display_suppresses_title_from_typed_tool_name_not_title_text() -> Result<()> {
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
                title: "edit notes/todo.md".to_string(),
                sections: vec![ToolDisplaySection {
                    label: "notes/todo.md".to_string(),
                    kind: ContentKind::Diff {
                        language: "diff".to_string(),
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
        "typed edit identity must suppress the redundant title row; got {lines:?}"
    );
    assert!(
        !lines
            .iter()
            .any(|line| line.contains("notes/todo.md (diff)")),
        "single-section edit display must also skip the redundant section label; got {lines:?}"
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
                title: "edit sample.txt".to_string(),
                sections: vec![
                    ToolDisplaySection {
                        label: "sample.txt".to_string(),
                        kind: ContentKind::Diff {
                            language: "diff".to_string(),
                        },
                        content: "--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n"
                            .to_string(),
                        stats: None,
                    },
                    ToolDisplaySection {
                        label: "other.txt".to_string(),
                        kind: ContentKind::Diff {
                            language: "diff".to_string(),
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
                title: "nu sample.rs".to_string(),
                sections: vec![ToolDisplaySection {
                    label: "sample.rs".to_string(),
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
fn edit_preview_display_omits_redundant_single_file_stats_line() {
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
                title: "edit sample.txt".to_string(),
                sections: vec![ToolDisplaySection {
                    label: "sample.txt".to_string(),
                    kind: ContentKind::Diff {
                        language: "diff".to_string(),
                    },
                    content: "--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n"
                        .to_string(),
                    stats: Some(nu_agent_core::protocol::event::ToolDisplayStats {
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
    assert!(!lines.iter().any(|line| line.starts_with("files=")));
    assert!(!lines.iter().any(|line| line.contains("+3 -1")));
}

/// The suppression decision is keyed on the TYPED tool identity: a NON-edit
/// tool whose display happens to carry an edit-shaped single diff section
/// (or an edit-prefixed title) must keep its title and section label — the
/// display shape alone never triggers suppression.
#[test]
fn non_edit_tool_with_edit_shaped_display_keeps_title_and_label() {
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
                title: "edit bar.rs".to_string(),
                sections: vec![ToolDisplaySection {
                    label: "changes".to_string(),
                    kind: ContentKind::Diff {
                        language: "diff".to_string(),
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
