use super::*;

// ── BlockSource::project ────────────────────────────────────────────────────

/// Core projection: markdown through the shared pipeline, the tool call line,
/// the nameless tool fallback, diff/code preview sections, and the plain
/// preview section.
#[test]
fn project_core_table() -> Result<()> {
    // -- Setup & Fixtures
    let markdown = BlockSource::Markdown {
        role: MessageRole::Assistant,
        markdown: "# Title\n\nbody **bold**".to_string(),
    };
    let tool = BlockSource::Tool {
        name: ToolName("edit".to_string()),
        call: CallLine {
            summary: "→ foo.rs".to_string(),
        },
        preview: None,
    };
    let nameless = BlockSource::Tool {
        name: ToolName(String::new()),
        call: CallLine {
            summary: "→ foo.rs".to_string(),
        },
        preview: None,
    };
    let diff_display = Display {
        title: None,
        sections: vec![DisplaySection {
            label: None,
            kind: ContentKind::Diff {
                language: "diff".to_string(),
                path: "test.rs".to_string(),
            },
            content: "--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\n".to_string(),
            stats: None,
        }],
    };
    let code_display = Display {
        title: None,
        sections: vec![DisplaySection {
            label: None,
            kind: ContentKind::Code {
                language: "nu".to_string(),
            },
            content: "ls | select name".to_string(),
            stats: None,
        }],
    };
    let plain = BlockSource::Tool {
        name: ToolName("tool".to_string()),
        call: CallLine {
            summary: "→ out".to_string(),
        },
        preview: Some(Display {
            title: Some("plain".to_string()),
            sections: vec![DisplaySection {
                label: Some("out".to_string()),
                kind: ContentKind::Plain,
                content: "done".to_string(),
                stats: None,
            }],
        }),
    };

    // -- Exec
    let markdown_projected = markdown.project(80);
    let tool_projected = tool.project(80);
    let nameless_projected = nameless.project(80);
    let diff_projected = BlockSource::Tool {
        name: ToolName("edit".to_string()),
        call: CallLine {
            summary: "→ a.rs (diff)".to_string(),
        },
        preview: Some(diff_display),
    }
    .project(80);
    let code_projected = BlockSource::Tool {
        name: ToolName("nu".to_string()),
        call: CallLine {
            summary: "ls | select name".to_string(),
        },
        preview: Some(code_display),
    }
    .project(80);
    let plain_projected = plain.project(80);

    // -- Check: markdown projects through the shared pipeline.
    let expected_markdown =
        crate::transcript::markdown::render_markdown_lines("# Title\n\nbody **bold**", Some(80));
    assert_eq!(
        markdown_projected, expected_markdown,
        "markdown must project via the shared pipeline"
    );
    assert!(
        !markdown_projected.is_empty(),
        "markdown must produce lines"
    );

    // -- Check: the tool call line carries the name in emphasis and the summary in muted.
    let row0 = tool_projected.first().ok_or("row 0 must exist")?;
    assert_eq!(row0.spans.len(), 2, "name + summary spans");
    assert_eq!(row0.spans[0].text, "edit");
    assert_eq!(row0.spans[0].hint, StyleHint::Emphasis);
    assert_eq!(row0.spans[1].text, " → foo.rs");
    assert_eq!(row0.spans[1].hint, StyleHint::Muted);

    // -- Check: a nameless tool renders the summary span alone.
    let nameless_row0 = nameless_projected.first().ok_or("row 0 must exist")?;
    assert_eq!(
        nameless_row0.spans.len(),
        1,
        "nameless tool renders one span"
    );
    assert_eq!(nameless_row0.spans[0].text, "→ foo.rs");
    assert_eq!(nameless_row0.spans[0].hint, StyleHint::Muted);

    // -- Check: diff and code preview sections project through their helpers.
    let expected_diff = crate::transcript::markdown::project_diff_lines(
        "--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\n",
        "diff",
    );
    assert_eq!(diff_projected[1..], expected_diff[..], "diff preview lines");
    let added = diff_projected
        .iter()
        .find(|line| line.diff_tint == Some(DiffTint::Add))
        .ok_or("diff preview must carry an Add-tinted line")?;
    assert_eq!(added.spans[0].hint, StyleHint::Muted, "gutter span");
    assert!(
        added.spans.iter().all(|span| !span.text.ends_with('\n')),
        "diff body spans must not embed a trailing newline; got {:?}",
        added.spans
    );
    let expected_code =
        crate::transcript::markdown::project_code_block_lines("nu", "ls | select name");
    assert_eq!(code_projected[1..], expected_code[..], "code preview lines");

    // -- Check: a plain preview section projects to a single Normal line.
    let plain_text = projected_lines_text(&plain_projected);
    assert_eq!(plain_text[0], "tool → out", "call line");
    assert_eq!(plain_text[1], "plain", "title row");
    assert_eq!(plain_text[2], "out ()", "label row (Plain has no language)");
    assert_eq!(plain_text[3], "done", "content row");
    let preview_line = plain_projected.get(3).ok_or("preview line must exist")?;
    assert_eq!(preview_line.spans.len(), 1);
    assert_eq!(preview_line.spans[0].hint, StyleHint::Normal);
    Ok(())
}

/// A tool preview renders its display title, each section's
/// `label (language)` row, and its stats row.
#[test]
fn project_tool_preview_table() -> Result<()> {
    // -- Setup & Fixtures: a non-edit, non-nu tool with a single code section
    // (so neither the edit-title nor the nu-title suppression applies).
    let titled = BlockSource::Tool {
        name: ToolName("run".to_string()),
        call: CallLine {
            summary: "nu".to_string(),
        },
        preview: Some(Display {
            title: Some("edit foo.rs".to_string()),
            sections: vec![DisplaySection {
                label: Some("nu".to_string()),
                kind: ContentKind::Code {
                    language: "nu".to_string(),
                },
                content: "ls".to_string(),
                stats: None,
            }],
        }),
    };
    // Two sections so the single-section edit suppression does not apply; the
    // label differs from the call summary so a whole-line check cannot be
    // satisfied by the summary line.
    let labeled = BlockSource::Tool {
        name: ToolName("edit".to_string()),
        call: CallLine {
            summary: "→ foo.rs (diff)".to_string(),
        },
        preview: Some(Display {
            title: Some("edit foo.rs".to_string()),
            sections: vec![
                DisplaySection {
                    label: Some("alpha.rs".to_string()),
                    kind: ContentKind::Diff {
                        language: "diff".to_string(),
                        path: "test.rs".to_string(),
                    },
                    content: "--- a\n+++ b\n".to_string(),
                    stats: None,
                },
                DisplaySection {
                    label: Some("beta.rs".to_string()),
                    kind: ContentKind::Diff {
                        language: "diff".to_string(),
                        path: "test.rs".to_string(),
                    },
                    content: "--- c\n+++ d\n".to_string(),
                    stats: None,
                },
            ],
        }),
    };
    let with_stats = BlockSource::Tool {
        name: ToolName("run".to_string()),
        call: CallLine {
            summary: "nu".to_string(),
        },
        preview: Some(Display {
            title: Some("nu".to_string()),
            sections: vec![DisplaySection {
                label: Some("nu".to_string()),
                kind: ContentKind::Code {
                    language: "nu".to_string(),
                },
                content: "ls".to_string(),
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
    let titled_lines = projected_lines_text(&titled.project(80));
    let labeled_lines = projected_lines_text(&labeled.project(80));
    let stats_lines = projected_lines_text(&with_stats.project(80));

    // -- Check: the display title renders as its own line.
    assert!(
        titled_lines.iter().any(|l| l == "edit foo.rs"),
        "display title must render as its own line, got: {titled_lines:?}"
    );

    // -- Check: the section label renders as `label (language)`.
    assert!(
        labeled_lines.iter().any(|l| l == "alpha.rs (diff)"),
        "section label must render as `label (language)`, got: {labeled_lines:?}"
    );

    // -- Check: the stats row renders.
    assert!(
        stats_lines
            .iter()
            .any(|l| l.contains("files=2") && l.contains("+5") && l.contains("-1")),
        "stats line must render, got: {stats_lines:?}"
    );
    Ok(())
}
