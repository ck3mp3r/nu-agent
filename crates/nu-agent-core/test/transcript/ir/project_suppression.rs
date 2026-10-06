use super::*;

// ── BlockSource::project — title/label/stats suppression ────────────────────

/// WHEN the preview is a single-diff-section for an edit tool, THE title line
/// and the section label line SHALL be suppressed.
#[test]
fn project_tool_single_diff_edit_suppresses_title_and_label() -> Result<()> {
    // -- Setup & Fixtures: the label row would render as "changes (diff)",
    // which is distinct from the call summary "→ foo.rs (diff)", so a
    // whole-line check cannot be satisfied by the summary line.
    let source = BlockSource::Tool {
        name: ToolName("edit".to_string()),
        call: CallLine {
            summary: "→ foo.rs (diff)".to_string(),
        },
        preview: Some(Display {
            title: "edit foo.rs".to_string(),
            sections: vec![DisplaySection {
                label: "changes".to_string(),
                kind: ContentKind::Diff {
                    language: "diff".to_string(),
                },
                content: "--- a\n+++ b\n".to_string(),
                stats: None,
            }],
        }),
    };

    // -- Exec
    let projected = source.project(80);

    // -- Check: neither the title line nor the label line appears.
    let lines = projected_lines_text(&projected);
    assert!(
        !lines.iter().any(|l| l == "edit foo.rs"),
        "redundant edit title must be suppressed, got: {lines:?}"
    );
    assert!(
        !lines.iter().any(|l| l == "changes (diff)"),
        "redundant edit section label must be suppressed, got: {lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.contains("+++ b")),
        "the diff content must still render, got: {lines:?}"
    );
    Ok(())
}

/// WHEN a non-edit tool carries a single-diff-section preview, THE title and
/// label SHALL NOT be suppressed: suppression is keyed on TYPED tool identity
/// plus display shape, so display shape alone never triggers it.
#[test]
fn project_tool_non_edit_with_single_diff_preview_keeps_title_and_label() -> Result<()> {
    // -- Setup & Fixtures: an `edit`-shaped display (single diff section,
    // title text starting with "edit ") on a NON-edit tool. Shape and title
    // text must not influence the decision.
    let source = BlockSource::Tool {
        name: ToolName("run".to_string()),
        call: CallLine {
            summary: "→ foo.rs (diff)".to_string(),
        },
        preview: Some(Display {
            title: "edit foo.rs".to_string(),
            sections: vec![DisplaySection {
                label: "changes".to_string(),
                kind: ContentKind::Diff {
                    language: "diff".to_string(),
                },
                content: "--- a\n+++ b\n".to_string(),
                stats: None,
            }],
        }),
    };

    // -- Exec
    let projected = source.project(80);

    // -- Check: both the title line and the label line render.
    let lines = projected_lines_text(&projected);
    assert!(
        lines.iter().any(|l| l == "edit foo.rs"),
        "non-edit tool must keep its title, got: {lines:?}"
    );
    assert!(
        lines.iter().any(|l| l == "changes (diff)"),
        "non-edit tool must keep its section label, got: {lines:?}"
    );
    Ok(())
}

/// WHEN a `nu` tool carries a single-code-section preview, THE title row, the
/// section label row, and the stats row SHALL be suppressed: the call line
/// already shows the command, so `nu` / `nu (nu)` / stats add nothing.
#[test]
fn project_tool_single_code_nu_suppresses_title_label_and_stats() -> Result<()> {
    // -- Setup & Fixtures: the label row would render as "nu (nu)" and the
    // title as "nu", both distinct from the call summary "ls | select name",
    // so whole-line checks cannot be satisfied by the summary line.
    let source = BlockSource::Tool {
        name: ToolName("nu".to_string()),
        call: CallLine {
            summary: "ls | select name".to_string(),
        },
        preview: Some(Display {
            title: "nu".to_string(),
            sections: vec![DisplaySection {
                label: "nu".to_string(),
                kind: ContentKind::Code {
                    language: "nu".to_string(),
                },
                content: "ls | select name".to_string(),
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

    // -- Check: only the call line, then the highlighted command lines.
    let expected = crate::transcript::markdown::project_code_block_lines("nu", "ls | select name");
    assert_eq!(
        projected[1..],
        expected[..],
        "code lines follow the call line"
    );
    let lines = projected_lines_text(&projected);
    assert!(
        !lines.iter().any(|l| l == "nu"),
        "redundant nu title must be suppressed, got: {lines:?}"
    );
    assert!(
        !lines.iter().any(|l| l == "nu (nu)"),
        "redundant nu section label must be suppressed, got: {lines:?}"
    );
    assert!(
        !lines.iter().any(|l| l.contains("files=2")),
        "nu stats row must be suppressed, got: {lines:?}"
    );
    Ok(())
}

/// WHEN a `nu` tool carries an empty call summary and a single-code-section
/// preview, THE row 0 SHALL render the tool name alone with no trailing
/// space, followed by the highlighted command lines.
#[test]
fn project_tool_empty_summary_nu_renders_name_alone() -> Result<()> {
    // -- Setup & Fixtures: the production shape after the fix — the command
    // lives in the preview, the call line carries no summary.
    let source = BlockSource::Tool {
        name: ToolName("nu".to_string()),
        call: CallLine {
            summary: String::new(),
        },
        preview: Some(Display {
            title: "nu".to_string(),
            sections: vec![DisplaySection {
                label: "nu".to_string(),
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

    // -- Check: row 0 is exactly "nu" (no trailing space), then the code lines.
    let lines = projected_lines_text(&projected);
    assert_eq!(lines[0], "nu", "row 0 must be the tool name alone");
    let expected = crate::transcript::markdown::project_code_block_lines("nu", "ls | select name");
    assert_eq!(
        projected[1..],
        expected[..],
        "code lines follow the call line"
    );
    Ok(())
}

/// WHEN a `nu` tool carries a preview with more than one section, THE title
/// and every section label SHALL render: suppression is keyed on the
/// single-code-section shape, so a multi-section nu display keeps its rows.
#[test]
fn project_tool_multi_section_nu_keeps_title_and_labels() -> Result<()> {
    // -- Setup & Fixtures: two code sections, so the single-section shape
    // query is false.
    let source = BlockSource::Tool {
        name: ToolName("nu".to_string()),
        call: CallLine {
            summary: "ls".to_string(),
        },
        preview: Some(Display {
            title: "nu".to_string(),
            sections: vec![
                DisplaySection {
                    label: "first".to_string(),
                    kind: ContentKind::Code {
                        language: "nu".to_string(),
                    },
                    content: "ls".to_string(),
                    stats: None,
                },
                DisplaySection {
                    label: "second".to_string(),
                    kind: ContentKind::Code {
                        language: "nu".to_string(),
                    },
                    content: "pwd".to_string(),
                    stats: None,
                },
            ],
        }),
    };

    // -- Exec
    let projected = source.project(80);

    // -- Check
    let lines = projected_lines_text(&projected);
    assert_eq!(lines[1], "nu", "title row must render");
    assert!(
        lines.iter().any(|l| l == "first (nu)"),
        "first section label must render, got: {lines:?}"
    );
    assert!(
        lines.iter().any(|l| l == "second (nu)"),
        "second section label must render, got: {lines:?}"
    );
    Ok(())
}

/// WHEN a non-nu tool carries a single-code-section preview, THE title and
/// label SHALL NOT be suppressed: suppression is keyed on TYPED tool identity
/// plus display shape, so display shape alone never triggers it.
#[test]
fn project_tool_non_nu_with_single_code_preview_keeps_title_and_label() -> Result<()> {
    // -- Setup & Fixtures: a `nu`-shaped display (single code section, title
    // "nu") on a non-nu tool. Shape and title text must not influence the
    // decision.
    let source = BlockSource::Tool {
        name: ToolName("run".to_string()),
        call: CallLine {
            summary: "ls".to_string(),
        },
        preview: Some(Display {
            title: "nu".to_string(),
            sections: vec![DisplaySection {
                label: "nu".to_string(),
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

    // -- Check
    let lines = projected_lines_text(&projected);
    assert_eq!(lines[1], "nu", "title row must render");
    assert_eq!(lines[2], "nu (nu)", "label row must render");
    Ok(())
}
