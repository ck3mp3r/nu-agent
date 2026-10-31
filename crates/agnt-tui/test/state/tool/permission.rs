use super::*;

// ---------------------------------------------------------------------------
// Permission pre-display and tool preview dispatch
// ---------------------------------------------------------------------------

/// A permission request with a display sets the pending tool entry's preview
/// in place; the match is by `tool_key`, so a decorated tool name still
/// attaches. An unmatched key or a missing display adds nothing.
#[test]
fn permission_request_display_matrix() -> Result<()> {
    // -- Setup & Fixtures
    let diff = "+new content";

    // -- Exec & Check: the display lands on the tool entry's preview.
    let mut state = AppState::default();
    reduce_ui(&mut state, started("edit", r#"{"file":"foo.rs"}"#));
    apply_permission_request_display(
        &mut state,
        &permission_context(
            "edit(file=foo.rs)",
            "edit\n{\"file\":\"foo.rs\"}",
            Some(ToolDisplay {
                title: Some("edit foo.rs".to_string()),
                sections: vec![diff_section(Some("changes"), diff)],
            }),
        ),
    );
    assert_eq!(state.transcript.len(), 1, "the preview lives on the entry");
    let entry = state
        .transcript
        .entries()
        .first()
        .ok_or("should have tool entry")?;
    let TranscriptEntry::Tool(tool) = entry else {
        return Err("expected Tool entry".into());
    };
    assert!(tool.call.call_line.summary.contains("foo.rs"));
    let preview = tool.preview.as_ref().ok_or("should have preview")?;
    assert!(display_lines(preview).contains(diff));

    // -- Exec & Check: decorated tool name still matches via tool_key.
    let mut decorated = AppState::default();
    let arguments =
        r#"{"path":"foo.rs","mode":"apply","operation":{"type":"create","content":"hi\n"}}"#;
    reduce_ui(&mut decorated, started("edit", arguments));
    apply_permission_request_display(
        &mut decorated,
        &permission_context(
            "edit(mode=apply, operation={...}, path=foo.rs)",
            &format!("edit\n{arguments}"),
            Some(ToolDisplay {
                title: Some("edit foo.rs".to_string()),
                sections: vec![diff_section(Some("changes"), "+hi")],
            }),
        ),
    );
    assert_eq!(decorated.transcript.len(), 1);
    let entry = decorated
        .transcript
        .entries()
        .first()
        .ok_or("should have tool entry")?;
    let TranscriptEntry::Tool(tool) = entry else {
        return Err("expected Tool entry".into());
    };
    let preview = tool.preview.as_ref().ok_or("should have preview")?;
    assert!(display_lines(preview).contains("+hi"));

    // -- Exec & Check: unmatched tool_key leaves the entry without a preview.
    let mut unmatched = AppState::default();
    reduce_ui(&mut unmatched, started("edit", r#"{"path":"foo.rs"}"#));
    apply_permission_request_display(
        &mut unmatched,
        &permission_context(
            "edit(path=other.rs)",
            "edit\n{\"path\":\"other.rs\"}",
            Some(ToolDisplay {
                title: Some("edit other.rs".to_string()),
                sections: vec![],
            }),
        ),
    );
    assert_eq!(unmatched.transcript.len(), 1);
    let entry = unmatched
        .transcript
        .entries()
        .first()
        .ok_or("should have tool entry")?;
    let TranscriptEntry::Tool(tool) = entry else {
        return Err("expected Tool entry".into());
    };
    assert!(tool.preview.is_none());

    // -- Exec & Check: a request without a display adds no entries.
    let mut no_display = AppState::default();
    reduce_ui(&mut no_display, started("nu", r#"{"command":"ls"}"#));
    let len_after_start = no_display.transcript.len();
    no_display
        .permission
        .open_prompt(PermissionPrompt::from_context(
            "req-1",
            &permission_context("nu(command=ls)", "nu\n{\"command\":\"ls\"}", None),
        ));
    assert_eq!(no_display.transcript.len(), len_after_start);

    // -- Setup & Fixtures: the display dedup contract.
    let display = || ToolDisplay {
        title: Some("edit bar.rs".to_string()),
        sections: vec![diff_section(Some("changes"), "+new content")],
    };

    // -- Exec & Check: source-suppressed display shows once.
    let mut suppressed = AppState::default();
    reduce_ui(&mut suppressed, started("edit", r#"{"file":"bar.rs"}"#));
    apply_permission_request_display(
        &mut suppressed,
        &permission_context(
            "edit(file=bar.rs)",
            "edit\n{\"file\":\"bar.rs\"}",
            Some(display()),
        ),
    );
    complete_with_display(&mut suppressed, "edit", r#"{"file":"bar.rs"}"#, None);
    let lines: Vec<String> = suppressed
        .transcript
        .entries()
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.contains("+new content"))
            .count(),
        1
    );

    // -- Exec & Check: no local dedup — a resent display is recorded again.
    // The completion writes the result slot even when the gate preview showed
    // the same content; the unified display (`result.or(preview)`) still
    // renders once, because the result takes over the preview.
    let mut resent = AppState::default();
    reduce_ui(&mut resent, started("edit", r#"{"file":"bar.rs"}"#));
    apply_permission_request_display(
        &mut resent,
        &permission_context(
            "edit(file=bar.rs)",
            "edit\n{\"file\":\"bar.rs\"}",
            Some(display()),
        ),
    );
    complete_with_display(&mut resent, "edit", r#"{"file":"bar.rs"}"#, Some(display()));
    assert!(
        tool_entry_at(&resent, 0)?.result.is_some(),
        "the resent completion display lands on the result slot; the TUI does not dedup"
    );
    let lines: Vec<String> = resent
        .transcript
        .entries()
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.contains("+new content"))
            .count(),
        1,
        "the result takes over the preview: one rendered display"
    );

    // -- Exec & Check: no prior permission → display pushes normally.
    let mut plain = AppState::default();
    reduce_ui(&mut plain, started("edit", r#"{"path":"bar.rs"}"#));
    complete_with_display(
        &mut plain,
        "edit",
        r#"{"path":"bar.rs"}"#,
        Some(ToolDisplay {
            title: None,
            sections: vec![diff_section(None, "+new content")],
        }),
    );
    let lines: Vec<String> = plain
        .transcript
        .entries()
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();
    assert!(!lines.iter().any(|line| line.contains("changes (diff)")));
    assert!(lines.iter().any(|line| line.contains("+new content")));
    Ok(())
}
