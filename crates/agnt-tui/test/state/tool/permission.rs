use super::*;

// ---------------------------------------------------------------------------
// Permission pre-display and tool preview dispatch
// ---------------------------------------------------------------------------

/// A permission request with a display pushes its own ToolDisplay block after
/// the pending Tool block; the match is by `tool_key`, so a decorated tool name
/// still attaches. An unmatched key or a missing display adds nothing.
#[test]
fn permission_request_display_matrix() -> Result<()> {
    // -- Setup & Fixtures
    let diff = "+new content";

    // -- Exec & Check: display pushes a preview block.
    let mut state = AppState::default();
    reduce_tool(&mut state, started("edit", r#"{"file":"foo.rs"}"#));
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
    assert_eq!(state.transcript.len(), 2);
    let tool_block = state
        .transcript
        .blocks()
        .first()
        .ok_or("should have tool block")?;
    let BlockSource::Tool { call, preview, .. } = &tool_block.source else {
        return Err("expected Tool block".into());
    };
    assert!(call.summary.contains("foo.rs"));
    assert!(preview.is_none());
    assert_eq!(tool_block.fill, Fill::None);
    let preview_block = state
        .transcript
        .blocks()
        .get(1)
        .ok_or("should have preview block")?;
    assert!(matches!(
        preview_block.source,
        BlockSource::ToolDisplay { .. }
    ));
    assert_eq!(preview_block.fill, Fill::Code);
    assert!(tool_display_lines(preview_block).contains(diff));

    // -- Exec & Check: decorated tool name still matches via tool_key.
    let mut decorated = AppState::default();
    let arguments =
        r#"{"path":"foo.rs","mode":"apply","operation":{"type":"create","content":"hi\n"}}"#;
    reduce_tool(&mut decorated, started("edit", arguments));
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
    assert_eq!(decorated.transcript.len(), 2);
    let preview_block = decorated
        .transcript
        .blocks()
        .get(1)
        .ok_or("should have preview block")?;
    assert!(tool_display_lines(preview_block).contains("+hi"));

    // -- Exec & Check: unmatched tool_key leaves the transcript unchanged.
    let mut unmatched = AppState::default();
    reduce_tool(&mut unmatched, started("edit", r#"{"path":"foo.rs"}"#));
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
    let BlockSource::Tool { preview, .. } = &unmatched.transcript.blocks()[0].source else {
        return Err("expected Tool block".into());
    };
    assert!(preview.is_none());

    // -- Exec & Check: a request without a display adds no entries.
    let mut no_display = AppState::default();
    reduce_tool(&mut no_display, started("nu", r#"{"command":"ls"}"#));
    let len_after_start = no_display.transcript.len();
    no_display
        .permission
        .reduce_permission_event(agnt_core::bus::PermissionEvent::Requested {
            request_id: "req-1".to_string(),
            context: Box::new(permission_context(
                "nu(command=ls)",
                "nu\n{\"command\":\"ls\"}",
                None,
            )),
        });
    assert_eq!(no_display.transcript.len(), len_after_start);

    // -- Setup & Fixtures: the display dedup contract.
    let display = || ToolDisplay {
        title: Some("edit bar.rs".to_string()),
        sections: vec![diff_section(Some("changes"), "+new content")],
    };

    // -- Exec & Check: source-suppressed display shows once.
    let mut suppressed = AppState::default();
    reduce_tool(&mut suppressed, started("edit", r#"{"file":"bar.rs"}"#));
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
        .blocks()
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

    // -- Exec & Check: no local dedup — a resent display shows twice.
    let mut resent = AppState::default();
    reduce_tool(&mut resent, started("edit", r#"{"file":"bar.rs"}"#));
    apply_permission_request_display(
        &mut resent,
        &permission_context(
            "edit(file=bar.rs)",
            "edit\n{\"file\":\"bar.rs\"}",
            Some(display()),
        ),
    );
    complete_with_display(&mut resent, "edit", r#"{"file":"bar.rs"}"#, Some(display()));
    let lines: Vec<String> = resent
        .transcript
        .blocks()
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.contains("+new content"))
            .count(),
        2
    );

    // -- Exec & Check: no prior permission → display pushes normally.
    let mut plain = AppState::default();
    reduce_tool(&mut plain, started("edit", r#"{"path":"bar.rs"}"#));
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
        .blocks()
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();
    assert!(!lines.iter().any(|line| line.contains("changes (diff)")));
    assert!(lines.iter().any(|line| line.contains("+new content")));
    Ok(())
}
