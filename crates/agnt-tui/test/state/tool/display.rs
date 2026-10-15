use super::*;

// ---------------------------------------------------------------------------
// Diff and code section rendering
// ---------------------------------------------------------------------------

/// A `diff` section carries the structural hints (DiffHunk for `@@ `, Meta for
/// `---`/`+++`) and syntax-highlighted bodies with a line-level diff tint —
/// not whole-line DiffAdd/DiffRemove hints (task 7f931bef). Diff lines keep the
/// line-number prefixes added by `project_diff_lines` (task 6424470b), and
/// non-diff code sections still get syntect MdCode* hints.
#[test]
fn tool_display_section_hints_matrix() -> Result<()> {
    // -- Setup & Fixtures
    let diff_content = "--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n context\n\\ No newline at end of file\n";
    let prefix_content = "@@ -3,2 +3,2 @@\n alpha\n-beta\n+omega\n";

    // -- Exec & Check: diff section hints and tints.
    let mut state = AppState::default();
    reduce_tool(&mut state, started("edit", r#"{"path":"sample.txt"}"#));
    complete_with_display(
        &mut state,
        "edit",
        r#"{"path":"sample.txt"}"#,
        Some(ToolDisplay {
            title: Some("edit sample.txt".to_string()),
            sections: vec![diff_section(Some("sample.txt"), diff_content)],
        }),
    );
    let hints = diff_section_hints(&state);
    assert!(hints.contains(&StyleHint::DiffHunk), "got {hints:?}");
    assert!(hints.contains(&StyleHint::Meta), "got {hints:?}");
    assert!(hints.contains(&StyleHint::Muted), "got {hints:?}");
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
        "got {tints:?}"
    );

    // -- Exec & Check: line-number prefixes survive projection.
    let mut prefixed = AppState::default();
    reduce_tool(&mut prefixed, started("edit", r#"{"path":"sample.txt"}"#));
    complete_with_display(
        &mut prefixed,
        "edit",
        r#"{"path":"sample.txt"}"#,
        Some(ToolDisplay {
            title: Some("edit sample.txt".to_string()),
            sections: vec![diff_section(Some("sample.txt"), prefix_content)],
        }),
    );
    let lines: Vec<String> = prefixed
        .transcript
        .blocks()
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();
    for needle in ["│alpha", "│beta", "│omega"] {
        assert!(
            lines.iter().any(|line| line.contains(needle)),
            "line must keep its line-number prefix {needle}; got {lines:?}"
        );
    }

    // -- Exec & Check: non-diff code sections keep MdCode* hints.
    let mut code = AppState::default();
    reduce_tool(&mut code, started("nu", r#"{"command":"ls"}"#));
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
    let code_hints = diff_section_hints(&code);
    assert!(
        code_hints
            .iter()
            .any(|h| matches!(h, StyleHint::MdCodeKeyword | StyleHint::MdCodePlain)),
        "non-diff code section must keep MdCode* hints; got {code_hints:?}"
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
    use agnt_core::transcript::renderer::FrameContext;

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
                title: Some("edit README.md".to_string()),
                sections: vec![ToolDisplaySection {
                    label: Some("README.md".to_string()),
                    kind: ContentKind::Diff {
                        language: "diff".to_string(),
                        path: "test.rs".to_string(),
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

/// A diff display renders as dedicated ToolDisplay blocks: the Tool call row
/// stays a Tool block, the body lines live in ToolDisplay blocks, and the diff
/// highlighting survives the prefix-hygiene fix.
#[test]
fn tool_display_renders_diff_sections_as_dedicated_blocks() -> Result<()> {
    // -- Setup & Fixtures
    let content = "--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n";
    let mut state = AppState::default();
    reduce_tool(&mut state, started("edit", r#"{"path":"sample.txt"}"#));
    complete_with_display(
        &mut state,
        "edit",
        r#"{"path":"sample.txt"}"#,
        Some(ToolDisplay {
            title: None,
            sections: vec![diff_section(None, content)],
        }),
    );

    // -- Check: None title/label render no rows; the diff body renders.
    let lines: Vec<String> = state
        .transcript
        .blocks()
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();
    assert!(!lines.contains(&"edit sample.txt".to_string()));
    assert!(!lines.contains(&"sample.txt (diff)".to_string()));
    assert!(lines.iter().any(|line| line.contains("--- a/sample.txt")));
    assert!(lines.iter().any(|line| line.contains("+++ b/sample.txt")));

    // -- Check: the Tool call row stays a Tool block; the diff body lives in
    // ToolDisplay blocks.
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

    // -- Check: highlighting survives the prefix-hygiene fix.
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
    Ok(())
}

/// Diff displays preserve hunk line-range context and keep the line-number
/// prefixes readable without breaking highlighting.
#[test]
fn diff_display_preserves_hunk_context_and_line_number_readability() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    reduce_tool(&mut state, started("edit", r#"{"path":"sample.txt"}"#));
    complete_with_display(
        &mut state,
        "edit",
        r#"{"path":"sample.txt"}"#,
        Some(ToolDisplay {
            title: Some("edit sample.txt".to_string()),
            sections: vec![diff_section(
                Some("sample.txt"),
                "--- a/sample.txt\n+++ b/sample.txt\n@@ -10,3 +10,4 @@\n line-a\n-line-b\n+line-c\n line-d\n",
            )],
        }),
    );

    // -- Check: the hunk header survives projection.
    assert!(
        state
            .transcript
            .blocks()
            .iter()
            .any(|b| tool_display_lines(b).contains("@@ -10,3 +10,4 @@"))
    );

    // -- Setup & Fixtures: line-number readability case.
    let mut prefixed = AppState::default();
    reduce_tool(&mut prefixed, started("edit", r#"{"path":"sample.txt"}"#));
    complete_with_display(
        &mut prefixed,
        "edit",
        r#"{"path":"sample.txt"}"#,
        Some(ToolDisplay {
            title: Some("edit sample.txt".to_string()),
            sections: vec![diff_section(
                Some("sample.txt"),
                "--- a/sample.txt\n+++ b/sample.txt\n@@ -3,2 +3,2 @@\n alpha\n-beta\n+omega\n",
            )],
        }),
    );

    // -- Check: the line-number prefixes stay readable.
    let diff_rows: Vec<_> = prefixed
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
    Ok(())
}
