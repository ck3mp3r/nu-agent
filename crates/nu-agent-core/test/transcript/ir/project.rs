use super::*;

// ── BlockSource::project ────────────────────────────────────────────────────

/// Markdown projects through the shared pulldown-cmark pipeline at the
/// canvas width — identical to calling the helper directly.
#[test]
fn project_markdown_matches_render_markdown_lines() -> Result<()> {
    // -- Setup & Fixtures
    let source = BlockSource::Markdown {
        role: MessageRole::Assistant,
        markdown: "# Title\n\nbody **bold**".to_string(),
    };

    // -- Exec
    let projected = source.project(80);

    // -- Check
    let expected =
        crate::transcript::markdown::render_markdown_lines("# Title\n\nbody **bold**", Some(80));
    assert_eq!(
        projected, expected,
        "markdown must project via the shared pipeline"
    );
    assert!(!projected.is_empty(), "markdown must produce lines");
    Ok(())
}

/// The tool call line carries the name in emphasis and the summary in muted.
#[test]
fn project_tool_call_line_has_emphasis_name_and_muted_summary() -> Result<()> {
    // -- Setup & Fixtures
    let source = BlockSource::Tool {
        name: ToolName("edit".to_string()),
        call: CallLine {
            summary: "→ foo.rs".to_string(),
        },
        preview: None,
    };

    // -- Exec
    let projected = source.project(80);

    // -- Check
    let row0 = projected.first().ok_or("row 0 must exist")?;
    assert_eq!(row0.spans.len(), 2, "name + summary spans");
    assert_eq!(row0.spans[0].text, "edit");
    assert_eq!(row0.spans[0].hint, StyleHint::Emphasis);
    assert_eq!(row0.spans[1].text, " → foo.rs");
    assert_eq!(row0.spans[1].hint, StyleHint::Muted);
    Ok(())
}

/// A nameless tool block projects the summary span alone.
#[test]
fn project_nameless_tool_renders_summary_span_only() -> Result<()> {
    // -- Setup & Fixtures
    let source = BlockSource::Tool {
        name: ToolName(String::new()),
        call: CallLine {
            summary: "→ foo.rs".to_string(),
        },
        preview: None,
    };

    // -- Exec
    let projected = source.project(80);

    // -- Check
    let row0 = projected.first().ok_or("row 0 must exist")?;
    assert_eq!(row0.spans.len(), 1, "nameless tool renders one span");
    assert_eq!(row0.spans[0].text, "→ foo.rs");
    assert_eq!(row0.spans[0].hint, StyleHint::Muted);
    Ok(())
}

/// A diff preview section projects through the diff helper; a code preview
/// section through the code-block helper.
#[test]
fn project_tool_diff_and_code_preview_sections() -> Result<()> {
    // -- Setup & Fixtures
    let diff_display = Display {
        title: "edit a.rs".to_string(),
        sections: vec![DisplaySection {
            label: "a.rs".to_string(),
            kind: ContentKind::Diff {
                language: "diff".to_string(),
            },
            content: "--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\n".to_string(),
            stats: None,
        }],
    };
    let code_display = Display {
        title: "nu".to_string(),
        sections: vec![DisplaySection {
            label: "nu".to_string(),
            kind: ContentKind::Code {
                language: "nu".to_string(),
            },
            content: "ls | select name".to_string(),
            stats: None,
        }],
    };

    // -- Exec
    let diff_source = BlockSource::Tool {
        name: ToolName("edit".to_string()),
        call: CallLine {
            summary: "→ a.rs (diff)".to_string(),
        },
        preview: Some(diff_display),
    };
    let diff_projected = diff_source.project(80);
    let code_source = BlockSource::Tool {
        name: ToolName("nu".to_string()),
        call: CallLine {
            summary: "ls | select name".to_string(),
        },
        preview: Some(code_display),
    };
    let code_projected = code_source.project(80);

    // -- Check: call line first, then helper-projected preview lines.
    // The diff display is a single-diff-section `edit`, so its title and
    // section label are suppressed: the diff lines follow the call line
    // directly.
    let expected_diff = crate::transcript::markdown::project_diff_lines(
        "--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\n",
        "diff",
    );
    assert_eq!(diff_projected[1..], expected_diff[..], "diff preview lines");
    // The diff preview carries the line-level tint and a muted gutter, so the
    // renderer can paint the diff background while the body keeps syntax
    // colours (task 7f931bef).
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
    // The code display is a `nu` tool with a single code section: its title
    // and section label are suppressed, so the highlighted command lines
    // follow the call line directly.
    let expected_code =
        crate::transcript::markdown::project_code_block_lines("nu", "ls | select name");
    assert_eq!(code_projected[1..], expected_code[..], "code preview lines");
    Ok(())
}

/// A plain preview section projects to a single Normal content line.
#[test]
fn project_tool_plain_preview_section_is_normal_line() -> Result<()> {
    // -- Setup & Fixtures
    let source = BlockSource::Tool {
        name: ToolName("tool".to_string()),
        call: CallLine {
            summary: "→ out".to_string(),
        },
        preview: Some(Display {
            title: "plain".to_string(),
            sections: vec![DisplaySection {
                label: "out".to_string(),
                kind: ContentKind::Plain,
                content: "done".to_string(),
                stats: None,
            }],
        }),
    };

    // -- Exec
    let projected = source.project(80);

    // -- Check: call line, title, label, then the plain content line. The
    // `tool` name is not `edit`, so no suppression applies.
    let lines = projected_lines_text(&projected);
    assert_eq!(lines[0], "tool → out", "call line");
    assert_eq!(lines[1], "plain", "title row");
    assert_eq!(lines[2], "out ()", "label row (Plain has no language)");
    assert_eq!(lines[3], "done", "content row");
    let preview_line = projected.get(3).ok_or("preview line must exist")?;
    assert_eq!(preview_line.spans.len(), 1);
    assert_eq!(preview_line.spans[0].hint, StyleHint::Normal);
    Ok(())
}

/// WHEN `project` renders a Tool preview Display titled "edit foo.rs", THE
/// output SHALL contain the title text.
#[test]
fn project_tool_preview_includes_display_title() -> Result<()> {
    // -- Setup & Fixtures: a non-edit, non-nu tool with a single code section
    // (so neither the edit-title nor the nu-title suppression applies).
    let source = BlockSource::Tool {
        name: ToolName("run".to_string()),
        call: CallLine {
            summary: "nu".to_string(),
        },
        preview: Some(Display {
            title: "edit foo.rs".to_string(),
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

    // -- Check: a whole line equals the title.
    let lines = projected_lines_text(&projected);
    assert!(
        lines.iter().any(|l| l == "edit foo.rs"),
        "display title must render as its own line, got: {lines:?}"
    );
    Ok(())
}

/// WHEN `project` renders a Tool preview DisplaySection labeled "alpha.rs" with
/// `ContentKind::Diff`, THE output SHALL contain a line "alpha.rs (diff)".
#[test]
fn project_tool_preview_includes_section_label_with_language() -> Result<()> {
    // -- Setup & Fixtures: two sections so the single-section edit suppression
    // does not apply; the label differs from the call summary so a whole-line
    // check cannot be satisfied by the summary line.
    let source = BlockSource::Tool {
        name: ToolName("edit".to_string()),
        call: CallLine {
            summary: "→ foo.rs (diff)".to_string(),
        },
        preview: Some(Display {
            title: "edit foo.rs".to_string(),
            sections: vec![
                DisplaySection {
                    label: "alpha.rs".to_string(),
                    kind: ContentKind::Diff {
                        language: "diff".to_string(),
                    },
                    content: "--- a\n+++ b\n".to_string(),
                    stats: None,
                },
                DisplaySection {
                    label: "beta.rs".to_string(),
                    kind: ContentKind::Diff {
                        language: "diff".to_string(),
                    },
                    content: "--- c\n+++ d\n".to_string(),
                    stats: None,
                },
            ],
        }),
    };

    // -- Exec
    let projected = source.project(80);

    // -- Check
    let lines = projected_lines_text(&projected);
    assert!(
        lines.iter().any(|l| l == "alpha.rs (diff)"),
        "section label must render as `label (language)`, got: {lines:?}"
    );
    Ok(())
}

/// WHEN `project` renders a Tool preview DisplaySection with stats, THE
/// output SHALL contain the stats line.
#[test]
fn project_tool_preview_includes_stats_line() -> Result<()> {
    // -- Setup & Fixtures: a non-edit, non-nu tool with a single code section
    // (no stats suppression).
    let source = BlockSource::Tool {
        name: ToolName("run".to_string()),
        call: CallLine {
            summary: "nu".to_string(),
        },
        preview: Some(Display {
            title: "nu".to_string(),
            sections: vec![DisplaySection {
                label: "nu".to_string(),
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
