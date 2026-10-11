use super::*;

// ── BlockSource::project — producer-decided title/label rows ────────────────

/// WHEN a preview display carries `title: None`, THE title row SHALL NOT be
/// rendered: the producer decides the row is redundant.
#[test]
fn project_tool_display_with_no_title_omits_title_row() -> Result<()> {
    // -- Setup & Fixtures: the production edit shape — no title, no label.
    let source = BlockSource::Tool {
        name: ToolName("edit".to_string()),
        call: CallLine {
            summary: "→ foo.rs (diff)".to_string(),
        },
        preview: Some(Display {
            title: None,
            sections: vec![DisplaySection {
                label: None,
                kind: ContentKind::Diff {
                    language: "diff".to_string(),
                    path: "test.rs".to_string(),
                },
                content: "--- a\n+++ b\n".to_string(),
                stats: None,
            }],
        }),
    };

    // -- Exec
    let projected = source.project(80);

    // -- Check: the call line, then the diff lines; no title or label row.
    let lines = projected_lines_text(&projected);
    assert!(
        !lines.iter().any(|l| l == "edit foo.rs"),
        "a None title must render no title row, got: {lines:?}"
    );
    assert!(
        !lines.iter().any(|l| l == "changes (diff)"),
        "a None label must render no label row, got: {lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.contains("+++ b")),
        "the diff content must still render, got: {lines:?}"
    );
    Ok(())
}

/// WHEN a preview display carries `title: Some(...)`, THE title row SHALL be
/// rendered as its own line.
#[test]
fn project_tool_display_with_title_renders_title_row() -> Result<()> {
    // -- Setup & Fixtures
    let source = BlockSource::Tool {
        name: ToolName("run".to_string()),
        call: CallLine {
            summary: "nu".to_string(),
        },
        preview: Some(Display {
            title: Some("edit foo.rs".to_string()),
            sections: vec![DisplaySection {
                label: None,
                kind: ContentKind::Code {
                    language: "nu".to_string(),
                },
                content: "ls".to_string(),
                stats: None,
            }],
        }),
    };

    // -- Exec
    let projected = source.project(80);

    // -- Check: a whole line equals the title.
    let lines = projected_lines_text(&projected);
    assert!(
        lines.iter().any(|l| l == "edit foo.rs"),
        "a Some title must render as its own line, got: {lines:?}"
    );
    Ok(())
}

/// WHEN a section carries `label: Some(...)`, THE `label (language)` row SHALL
/// be rendered.
#[test]
fn project_tool_section_with_label_renders_label_row() -> Result<()> {
    // -- Setup & Fixtures: the label differs from the call summary so a
    // whole-line check cannot be satisfied by the summary line.
    let source = BlockSource::Tool {
        name: ToolName("edit".to_string()),
        call: CallLine {
            summary: "→ foo.rs (diff)".to_string(),
        },
        preview: Some(Display {
            title: None,
            sections: vec![DisplaySection {
                label: Some("alpha.rs".to_string()),
                kind: ContentKind::Diff {
                    language: "diff".to_string(),
                    path: "test.rs".to_string(),
                },
                content: "--- a\n+++ b\n".to_string(),
                stats: None,
            }],
        }),
    };

    // -- Exec
    let projected = source.project(80);

    // -- Check
    let lines = projected_lines_text(&projected);
    assert!(
        lines.iter().any(|l| l == "alpha.rs (diff)"),
        "a Some label must render as `label (language)`, got: {lines:?}"
    );
    Ok(())
}

/// WHEN a section carries `label: None`, THE label row SHALL NOT be rendered
/// even though the section content still renders.
#[test]
fn project_tool_section_without_label_omits_label_row() -> Result<()> {
    // -- Setup & Fixtures: the production nu shape — no title, no label.
    let source = BlockSource::Tool {
        name: ToolName("nu".to_string()),
        call: CallLine {
            summary: "⏱ 120s".to_string(),
        },
        preview: Some(Display {
            title: None,
            sections: vec![DisplaySection {
                label: None,
                kind: ContentKind::Code {
                    language: "nu".to_string(),
                },
                content: "ls | select name".to_string(),
                stats: None,
            }],
        }),
    };

    // -- Exec
    let projected = source.project(80);

    // -- Check: only the call line, then the highlighted command lines.
    let expected = crate::transcript::markdown::project_code_block_lines("nu", "ls | select name");
    assert_eq!(
        projected[1..],
        expected[..],
        "code lines follow the call line with no label row"
    );
    let lines = projected_lines_text(&projected);
    assert!(
        !lines.iter().any(|l| l == "nu (nu)"),
        "a None label must render no label row, got: {lines:?}"
    );
    Ok(())
}

/// WHEN a section carries stats, THE stats row SHALL be rendered regardless of
/// the title/label shape: the producer decides, the renderer never suppresses.
#[test]
fn project_tool_section_with_stats_renders_stats_row() -> Result<()> {
    // -- Setup & Fixtures
    let source = BlockSource::Tool {
        name: ToolName("edit".to_string()),
        call: CallLine {
            summary: "→ foo.rs (diff)".to_string(),
        },
        preview: Some(Display {
            title: None,
            sections: vec![DisplaySection {
                label: None,
                kind: ContentKind::Diff {
                    language: "diff".to_string(),
                    path: "test.rs".to_string(),
                },
                content: "--- a\n+++ b\n".to_string(),
                stats: Some(DisplayStats {
                    files_changed: Some(2),
                    insertions: Some(5),
                    deletions: Some(1),
                    diff_truncated: None,
                    omitted_files: None,
                    omitted_hunks: None,
                }),
            }],
        }),
    };

    // -- Exec
    let projected = source.project(80);

    // -- Check
    let lines = projected_lines_text(&projected);
    assert!(
        lines
            .iter()
            .any(|l| l.contains("files=2") && l.contains("+5") && l.contains("-1")),
        "stats line must render, got: {lines:?}"
    );
    Ok(())
}
