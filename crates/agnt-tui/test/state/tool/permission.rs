use super::*;

// ---------------------------------------------------------------------------
// Permission preview and tool preview dispatch
// ---------------------------------------------------------------------------

/// A permission request's preview content attaches to the pending call its
/// `tool_key` names; the card is derived from the call, and the content is the
/// one thing only the producer can compute. An unmatched key, or a request with
/// no content, adds nothing.
#[test]
fn permission_request_display_matrix() -> Result<()> {
    // -- Setup & Fixtures
    let diff = "+new content";

    // -- Exec & Check: the preview lands on the tool entry's preview slot.
    let mut state = AppState::default();
    reduce_ui(&mut state, started("edit", r#"{"path":"foo.rs"}"#));
    apply_permission_request_display(
        &mut state,
        &permission_context(
            "edit(path=foo.rs)",
            "edit\n{\"path\":\"foo.rs\"}",
            Some(diff),
        ),
    );
    assert_eq!(state.transcript.len(), 1, "the preview lives on the entry");
    let tool = tool_entry_at(&state, 0)?;
    assert!(
        call_line_of(tool).contains("foo.rs"),
        "the call line must derive from the call's own arguments"
    );
    let preview = tool.preview.as_ref().ok_or("should have preview")?;
    assert!(display_lines(preview).contains(diff));

    // -- Exec & Check: a decorated tool name still matches via tool_key.
    let mut decorated = AppState::default();
    let arguments =
        r#"{"path":"foo.rs","mode":"apply","operation":{"type":"create","content":"hi\n"}}"#;
    reduce_ui(&mut decorated, started("edit", arguments));
    apply_permission_request_display(
        &mut decorated,
        &permission_context(
            "edit(mode=apply, operation={...}, path=foo.rs)",
            &format!("edit\n{arguments}"),
            Some("+hi"),
        ),
    );
    assert_eq!(decorated.transcript.len(), 1);
    let tool = tool_entry_at(&decorated, 0)?;
    let preview = tool.preview.as_ref().ok_or("should have preview")?;
    assert!(display_lines(preview).contains("+hi"));

    // -- Exec & Check: an unmatched tool_key leaves the entry without a preview.
    let mut unmatched = AppState::default();
    reduce_ui(&mut unmatched, started("edit", r#"{"path":"foo.rs"}"#));
    apply_permission_request_display(
        &mut unmatched,
        &permission_context(
            "edit(path=other.rs)",
            "edit\n{\"path\":\"other.rs\"}",
            Some(diff),
        ),
    );
    assert_eq!(unmatched.transcript.len(), 1);
    let tool = tool_entry_at(&unmatched, 0)?;
    assert!(tool.preview.is_none());

    // -- Exec & Check: a request without content adds nothing.
    let mut no_content = AppState::default();
    reduce_ui(&mut no_content, started("nu", r#"{"command":"ls"}"#));
    let len_after_start = no_content.transcript.len();
    no_content
        .permission
        .open_prompt(PermissionPrompt::from_context(
            "req-1",
            &permission_context("nu(command=ls)", "nu\n{\"command\":\"ls\"}", None),
        ));
    assert_eq!(no_content.transcript.len(), len_after_start);

    // -- Exec & Check: the unified slot renders the body once when the gate
    // preview and the completion carry the same diff.
    let mut resent = AppState::default();
    reduce_ui(&mut resent, started("edit", r#"{"path":"bar.rs"}"#));
    apply_permission_request_display(
        &mut resent,
        &permission_context(
            "edit(path=bar.rs)",
            "edit\n{\"path\":\"bar.rs\"}",
            Some(diff),
        ),
    );
    complete_with_result(
        &mut resent,
        "edit",
        r#"{"path":"bar.rs"}"#,
        &edit_payload("bar.rs", diff),
    );
    assert!(
        tool_entry_at(&resent, 0)?.result.is_some(),
        "the completion body lands on the result slot"
    );
    let lines: Vec<String> = resent
        .transcript
        .entries()
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();
    assert_eq!(
        lines.iter().filter(|line| line.contains(diff)).count(),
        1,
        "the result takes over the preview: one rendered body"
    );
    Ok(())
}
