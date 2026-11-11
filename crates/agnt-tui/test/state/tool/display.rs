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

    // -- Exec & Check: the derived diff body carries the structural hints.
    let mut state = AppState::default();
    reduce_ui(&mut state, started("edit", r#"{"path":"sample.txt"}"#));
    complete_with_result(
        &mut state,
        "edit",
        r#"{"path":"sample.txt"}"#,
        &edit_payload("sample.txt", diff_content),
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
    reduce_ui(&mut prefixed, started("edit", r#"{"path":"sample.txt"}"#));
    complete_with_result(
        &mut prefixed,
        "edit",
        r#"{"path":"sample.txt"}"#,
        &edit_payload("sample.txt", prefix_content),
    );
    let lines: Vec<String> = prefixed
        .transcript
        .entries()
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();
    for needle in ["│alpha", "│beta", "│omega"] {
        assert!(
            lines.iter().any(|line| line.contains(needle)),
            "line must keep its line-number prefix {needle}; got {lines:?}"
        );
    }
    Ok(())
}

/// Regression (task 7bd175d2): every edit-display line must render as exactly
/// one visual row. A trailing newline in the projected row text made the word
/// wrapper emit an extra empty row after every diff line, interleaving blank
/// rows in the transcript.
#[test]
fn edit_display_renders_one_visual_row_per_line_without_blank_rows() -> Result<()> {
    let content = "--- a/README.md\n+++ b/README.md\n@@ -1,4 +1,6 @@\n # Title\n \n+```rust\n fn a() {}\n+```\n tail\n";
    let mut state = AppState::default();
    reduce_ui(&mut state, started("edit", r#"{"path":"README.md"}"#));
    complete_with_result(
        &mut state,
        "edit",
        r#"{"path":"README.md"}"#,
        &edit_payload("README.md", content),
    );

    let tool = tool_entry_at(&state, 0)?;
    let display = tool.result.as_ref().ok_or("should have result display")?;
    let display_lines = display.project_lines();
    assert!(
        display_lines.len() >= 8,
        "the entry must carry every diff line; got {}",
        display_lines.len()
    );

    let entry = state
        .transcript
        .entries()
        .first()
        .ok_or("should have tool entry")?;
    let probe = crate::widgets::RatatuiBackend::new(120, 0);
    let rendered_rows = agnt_render::RenderBackend::measure(&probe, entry, 120);
    assert_eq!(
        rendered_rows,
        display_lines.len() + 3,
        "the call row and the band's two margin rows bracket exactly one row per display line"
    );

    // No projected display line may carry a raw trailing newline.
    for line in &display_lines {
        let text: String = line.spans.iter().map(|span| span.text.as_str()).collect();
        assert!(
            !text.ends_with('\n'),
            "display line text must not embed a trailing newline; got {text:?}"
        );
    }
    Ok(())
}

/// One tool entry carries its diff display: the call row and the body live on
/// the same entry, the transcript gains no companion display entries, and the
/// diff highlighting survives the prefix-hygiene fix.
#[test]
fn tool_entry_carries_its_diff_display_once() -> Result<()> {
    // -- Setup & Fixtures
    let content = "--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n";
    let mut state = AppState::default();
    reduce_ui(&mut state, started("edit", r#"{"path":"sample.txt"}"#));
    complete_with_result(
        &mut state,
        "edit",
        r#"{"path":"sample.txt"}"#,
        &edit_payload("sample.txt", content),
    );

    // -- Check: one entry, carrying the result; no companion display entries.
    let entries = state.transcript.entries();
    assert_eq!(entries.len(), 1, "one entry per call");
    assert!(
        matches!(entries[0], TranscriptEntry::Tool(_)),
        "the call row and the body share the tool entry"
    );
    assert!(tool_entry_at(&state, 0)?.result.is_some());

    // -- Check: None title/label render no rows; the diff body renders.
    let lines: Vec<String> = entries
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();
    assert!(!lines.contains(&"edit sample.txt".to_string()));
    assert!(!lines.contains(&"sample.txt (diff)".to_string()));
    assert!(lines.iter().any(|line| line.contains("--- a/sample.txt")));
    assert!(lines.iter().any(|line| line.contains("+++ b/sample.txt")));

    // -- Check: highlighting survives the prefix-hygiene fix.
    let hints = diff_section_hints(&state);
    assert!(hints.contains(&StyleHint::Meta), "got {hints:?}");
    assert!(hints.contains(&StyleHint::DiffHunk), "got {hints:?}");
    Ok(())
}

/// Diff displays preserve hunk line-range context and keep the line-number
/// prefixes readable without breaking highlighting.
#[test]
fn diff_display_preserves_hunk_context_and_line_number_readability() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    reduce_ui(&mut state, started("edit", r#"{"path":"sample.txt"}"#));
    complete_with_result(
        &mut state,
        "edit",
        r#"{"path":"sample.txt"}"#,
        &edit_payload(
            "sample.txt",
            "--- a/sample.txt\n+++ b/sample.txt\n@@ -10,3 +10,4 @@\n line-a\n-line-b\n+line-c\n line-d\n",
        ),
    );

    // -- Check: the hunk header survives projection.
    assert!(
        state
            .transcript
            .entries()
            .iter()
            .flat_map(extract_all_text_from_entry)
            .any(|line| line.contains("@@ -10,3 +10,4 @@"))
    );

    // -- Setup & Fixtures: line-number readability case.
    let mut prefixed = AppState::default();
    reduce_ui(&mut prefixed, started("edit", r#"{"path":"sample.txt"}"#));
    complete_with_result(
        &mut prefixed,
        "edit",
        r#"{"path":"sample.txt"}"#,
        &edit_payload(
            "sample.txt",
            "--- a/sample.txt\n+++ b/sample.txt\n@@ -3,2 +3,2 @@\n alpha\n-beta\n+omega\n",
        ),
    );

    // -- Check: the line-number prefixes stay readable.
    let prefixed_lines: Vec<String> = prefixed
        .transcript
        .entries()
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();
    assert!(
        prefixed_lines.iter().any(|line| line.contains("│alpha")
            || line.contains("│beta")
            || line.contains("│omega"))
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// A completion without a display
// ---------------------------------------------------------------------------

/// WHEN a tool completes without a display, THE entry SHALL keep `result: None`
/// and THE transcript SHALL gain no extra entries — the pending preview, if
/// any, is what stays on screen.
#[test]
fn completion_without_display_leaves_no_result_and_no_extra_entries() -> Result<()> {
    // -- Setup & Fixtures: one completion whose tool draws no body.
    let mut bare = AppState::default();
    reduce_ui(&mut bare, started("read", r#"{"path":"a.rs"}"#));
    complete_with_result(&mut bare, "read", r#"{"path":"a.rs"}"#, "a.rs body");

    // -- Check: no result, no extra entries.
    assert_eq!(bare.transcript.len(), 1, "one entry per call");
    let tool = tool_entry_at(&bare, 0)?;
    assert!(
        tool.result.is_none(),
        "a tool that draws no body sets no result"
    );
    assert!(tool.preview.is_none(), "nothing was ever previewed");

    // -- Setup & Fixtures: a previewed call that completes drawing no body.
    let mut previewed = AppState::default();
    reduce_ui(&mut previewed, started("nu", r#"{"command":"ls"}"#));
    apply_permission_request_display(
        &mut previewed,
        &permission_requested("nu(command=ls)", "nu\n{\"command\":\"ls\"}", None),
    );
    complete_with_result(&mut previewed, "nu", r#"{"command":"ls"}"#, "out");

    // -- Check: the preview persists, no result is recorded, no entry is added.
    assert_eq!(previewed.transcript.len(), 1, "one entry per call");
    let tool = tool_entry_at(&previewed, 0)?;
    assert!(tool.result.is_none());
    let preview = tool.preview.as_ref().ok_or("should keep the preview")?;
    assert!(display_lines(preview).contains("ls"));
    Ok(())
}
