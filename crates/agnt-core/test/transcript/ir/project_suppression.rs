use super::*;

// ── BlockSource::project — producer-decided title/label rows ────────────────

/// The producer decides which rows are redundant: a `None` title or label
/// renders no row, a `Some` one renders its own line, and stats always render.
#[test]
fn project_tool_suppression_table() -> Result<()> {
    // -- Setup & Fixtures: the production edit shape — no title, no label.
    let no_title = BlockSource::Tool {
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
    let with_title = BlockSource::Tool {
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
    let with_label = BlockSource::Tool {
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
    // The production nu shape — no title, no label.
    let no_label = BlockSource::Tool {
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
    let with_stats = BlockSource::Tool {
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
    let no_title_lines = projected_lines_text(&no_title.project(80));
    let with_title_lines = projected_lines_text(&with_title.project(80));
    let with_label_lines = projected_lines_text(&with_label.project(80));
    let no_label_projected = no_label.project(80);
    let no_label_lines = projected_lines_text(&no_label_projected);
    let with_stats_lines = projected_lines_text(&with_stats.project(80));

    // -- Check: a None title and label render no rows; the diff still renders.
    assert!(
        !no_title_lines.iter().any(|l| l == "edit foo.rs"),
        "a None title must render no title row, got: {no_title_lines:?}"
    );
    assert!(
        !no_title_lines.iter().any(|l| l == "changes (diff)"),
        "a None label must render no label row, got: {no_title_lines:?}"
    );
    assert!(
        no_title_lines.iter().any(|l| l.contains("+++ b")),
        "the diff content must still render, got: {no_title_lines:?}"
    );

    // -- Check: a Some title renders as its own line.
    assert!(
        with_title_lines.iter().any(|l| l == "edit foo.rs"),
        "a Some title must render as its own line, got: {with_title_lines:?}"
    );

    // -- Check: a Some label renders as `label (language)`.
    assert!(
        with_label_lines.iter().any(|l| l == "alpha.rs (diff)"),
        "a Some label must render as `label (language)`, got: {with_label_lines:?}"
    );

    // -- Check: a None label renders no label row; code lines follow the call line.
    let expected_code =
        crate::transcript::markdown::project_code_block_lines("nu", "ls | select name");
    assert_eq!(
        no_label_projected[1..],
        expected_code[..],
        "code lines follow the call line with no label row"
    );
    assert!(
        !no_label_lines.iter().any(|l| l == "nu (nu)"),
        "a None label must render no label row, got: {no_label_lines:?}"
    );

    // -- Check: stats render regardless of the title/label shape.
    assert!(
        with_stats_lines
            .iter()
            .any(|l| l.contains("files=2") && l.contains("+5") && l.contains("-1")),
        "stats line must render, got: {with_stats_lines:?}"
    );
    Ok(())
}
