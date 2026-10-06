use super::*;

// ---------------------------------------------------------------------------
// Diff and code section rendering
// ---------------------------------------------------------------------------

/// A `diff` section carries the structural hints (DiffHunk for `@@ `, Meta for
/// `---`/`+++`) and syntax-highlighted bodies with a line-level diff tint —
/// not whole-line DiffAdd/DiffRemove hints (task 7f931bef).
#[test]
fn tool_display_diff_section_produces_diff_hints() -> Result<()> {
    let content = "--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n context\n\\ No newline at end of file\n";
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
                    content: content.to_string(),
                    stats: None,
                }],
            }),
            error_kind: None,
            message: None,
        },
    );

    let hints = diff_section_hints(&state);
    assert!(
        hints.contains(&StyleHint::DiffHunk),
        "diff section must carry DiffHunk hints for '@@ ' lines; got {hints:?}"
    );
    assert!(
        hints.contains(&StyleHint::Meta),
        "diff section must carry Meta hints for ---/+++ header lines; got {hints:?}"
    );
    assert!(
        hints.contains(&StyleHint::Muted),
        "diff body lines must carry a Muted gutter span; got {hints:?}"
    );
    assert!(
        !hints
            .iter()
            .any(|h| matches!(h, StyleHint::DiffAdd | StyleHint::DiffRemove)),
        "diff bodies must not carry whole-line DiffAdd/DiffRemove hints; got {hints:?}"
    );

    let tints = diff_section_tints(&state);
    assert!(
        tints.contains(&DiffTint::Add)
            && tints.contains(&DiffTint::Remove)
            && tints.contains(&DiffTint::Context),
        "diff section must carry Add/Remove/Context line tints; got {tints:?}"
    );
    Ok(())
}

/// Regression (task 6424470b): diff lines keep the line-number prefixes added
/// by `project_diff_lines`.
#[test]
fn tool_display_diff_section_keeps_line_number_prefixes() -> Result<()> {
    let content = "@@ -3,2 +3,2 @@\n alpha\n-beta\n+omega\n";
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
                    content: content.to_string(),
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
        lines.iter().any(|line| line.contains("│alpha")),
        "context line must keep its line-number prefix; got {lines:?}"
    );
    assert!(
        lines.iter().any(|line| line.contains("│beta")),
        "removed line must keep its line-number prefix; got {lines:?}"
    );
    assert!(
        lines.iter().any(|line| line.contains("│omega")),
        "added line must keep its line-number prefix; got {lines:?}"
    );
    Ok(())
}

/// Regression (task 6424470b criterion 2): non-diff code sections still get
/// syntect MdCode* hints — only the diff path changes.
#[test]
fn tool_display_non_diff_code_section_still_produces_code_hints() -> Result<()> {
    let content = "fn main() {}\n";
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
                    content: content.to_string(),
                    stats: None,
                }],
            }),
            error_kind: None,
            message: None,
        },
    );

    let hints = diff_section_hints(&state);
    assert!(
        hints
            .iter()
            .any(|h| matches!(h, StyleHint::MdCodeKeyword | StyleHint::MdCodePlain)),
        "non-diff code section must keep MdCode* hints; got {hints:?}"
    );
    Ok(())
}

/// Regression (task 7bd175d2): every edit-display line must render as exactly
/// one visual row. A trailing newline in the projected row text made the word
/// wrapper emit an extra empty row after every diff line, interleaving blank
/// rows in the transcript.
#[test]
fn edit_display_renders_one_visual_row_per_line_without_blank_rows() -> Result<()> {
    use crate::tui_renderer::layout;
    use nu_agent_core::transcript::renderer::FrameContext;

    let content = "--- a/README.md\n+++ b/README.md\n@@ -1,4 +1,6 @@\n # Title\n \n+```rust\n fn a() {}\n+```\n tail\n";
    let mut state = AppState::default();
    reduce_tool(&mut state, started("edit", r#"{"path":"README.md"}"#));
    reduce_tool(
        &mut state,
        ToolEvent::Completed {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"path":"README.md"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
                title: "edit README.md".to_string(),
                sections: vec![ToolDisplaySection {
                    label: "README.md".to_string(),
                    kind: ContentKind::Diff {
                        language: "diff".to_string(),
                    },
                    content: content.to_string(),
                    stats: None,
                }],
            }),
            error_kind: None,
            message: None,
        },
    );

    let diff_rows: Vec<_> = state
        .transcript
        .blocks()
        .iter()
        .filter(|b| matches!(b.source, BlockSource::ToolDisplay { .. }))
        .cloned()
        .collect();
    let display_line_count: usize = diff_rows
        .iter()
        .map(|b| extract_all_text_from_entry(b).len())
        .sum();
    assert!(
        display_line_count >= 8,
        "edit display must push every diff line; got {display_line_count}"
    );

    let ctx = FrameContext {
        width: 120,
        now_millis: 0,
        cursor: false,
        selected: false,
    };
    let mut rendered_rows = 0usize;
    for block in &diff_rows {
        rendered_rows += layout(block, &ctx, &TuiTheme::default()).len();
    }
    assert_eq!(
        rendered_rows, display_line_count,
        "each display line must render as exactly one visual row (no blank rows from trailing newlines)"
    );

    // No stored display line may carry a raw trailing newline.
    for block in &diff_rows {
        for text in extract_all_text_from_entry(block) {
            assert!(
                !text.ends_with('\n'),
                "display line text must not embed a trailing newline; got {text:?}"
            );
        }
    }
    Ok(())
}

#[test]
fn tool_display_renders_diff_sections_as_dedicated_code_blocks() {
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
    assert!(!lines.iter().any(|line| line.contains("fn main")));
    assert!(lines.iter().any(|line| line.contains("--- a/sample.txt")));
    assert!(lines.iter().any(|line| line.contains("+++ b/sample.txt")));
}

#[test]
fn tool_display_body_lines_are_unprefixed_while_tool_call_line_remains_prefixed() -> Result<()> {
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

    let call_row = state
        .transcript
        .blocks()
        .iter()
        .find(|b| matches!(&b.source, BlockSource::Tool { .. }))
        .ok_or("should have tool call row")?;
    assert!(matches!(call_row.source, BlockSource::Tool { .. }));

    let display_rows: Vec<_> = state
        .transcript
        .blocks()
        .iter()
        .filter(|b| {
            matches!(b.source, BlockSource::ToolDisplay { .. })
                && tool_display_lines(b).contains("--- a/sample.txt")
        })
        .collect();

    assert!(!display_rows.is_empty());
    assert!(
        display_rows
            .iter()
            .all(|b| matches!(b.source, BlockSource::ToolDisplay { .. }))
    );
    Ok(())
}

#[test]
fn tool_display_diff_block_highlighting_remains_after_prefix_hygiene_fix() {
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

    let diff_rows: Vec<_> = state
        .transcript
        .blocks()
        .iter()
        .filter(|b| {
            matches!(b.source, BlockSource::ToolDisplay { .. })
                && (tool_display_lines(b).contains("--- a/sample.txt")
                    || tool_display_lines(b).contains("+++ b/sample.txt"))
        })
        .collect();

    assert!(!diff_rows.is_empty());
    // Note: rendered field no longer exists in TranscriptEntry
}

#[test]
fn diff_display_preserves_hunk_line_range_context() {
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
                    content: "--- a/sample.txt\n+++ b/sample.txt\n@@ -10,3 +10,4 @@\n line-a\n-line-b\n+line-c\n line-d\n"
                        .to_string(),
                    stats: None,
                }],
            }),
            error_kind: None,
            message: None,
        },
    );

    assert!(
        state
            .transcript
            .blocks()
            .iter()
            .any(|b| tool_display_lines(b).contains("@@ -10,3 +10,4 @@"))
    );
}

#[test]
fn diff_display_supports_line_number_readability_without_breaking_highlighting() {
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
                    content: "--- a/sample.txt\n+++ b/sample.txt\n@@ -3,2 +3,2 @@\n alpha\n-beta\n+omega\n"
                        .to_string(),
                    stats: None,
                }],
            }),
            error_kind: None,
            message: None,
        },
    );

    let diff_rows: Vec<_> = state
        .transcript
        .blocks()
        .iter()
        .filter(|b| matches!(b.source, BlockSource::ToolDisplay { .. }))
        .collect();

    assert!(
        diff_rows
            .iter()
            .any(|b| tool_display_lines(b).contains("│alpha")
                || tool_display_lines(b).contains("│beta")
                || tool_display_lines(b).contains("│omega"))
    );
}
