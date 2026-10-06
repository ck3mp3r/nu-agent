use super::*;

// ---------------------------------------------------------------------------
// Permission pre-display and tool preview dispatch
// ---------------------------------------------------------------------------

#[test]
fn permission_requested_with_display_pushes_to_transcript() -> Result<()> {
    let mut state = AppState::default();

    reduce_tool(&mut state, started("edit", r#"{"file":"foo.rs"}"#));

    let context = nu_agent_core::protocol::event::PermissionRequestContext {
        tool: "edit(file=foo.rs)".to_string(),
        tool_key: "edit\n{\"file\":\"foo.rs\"}".to_string(),
        source: "closure".to_string(),
        mode: Some("apply".to_string()),
        matched_rule_identity: "tool:edit".to_string(),
        scope: "tool".to_string(),
        target_field: None,
        pattern: "edit".to_string(),
        summary: "→ {...}".to_string(),
        pre_authorize_display: Some(ToolDisplay {
            title: "edit foo.rs".to_string(),
            sections: vec![ToolDisplaySection {
                label: "changes".to_string(),
                kind: ContentKind::Diff {
                    language: "diff".to_string(),
                },
                content: "+new content".to_string(),
                stats: None,
            }],
        }),
    };
    apply_permission_request_display(&mut state, &context);

    // The preview is its own ToolDisplay block pushed directly after the
    // pending Tool block, so the store holds exactly two blocks.
    assert_eq!(state.transcript.len(), 2);
    let tool_block = state
        .transcript
        .blocks()
        .first()
        .ok_or("should have tool block")?;
    let BlockSource::Tool { call, preview, .. } = &tool_block.source else {
        panic!("expected Tool block");
    };
    assert!(
        call.summary.contains("foo.rs"),
        "call line must show the tool summary, got: {call:?}"
    );
    assert!(
        preview.is_none(),
        "the Tool block must keep preview None after the preview push"
    );
    assert_eq!(
        tool_block.fill,
        Fill::None,
        "the Tool block must keep Fill::None so the call line stays untinted"
    );

    let preview_block = state
        .transcript
        .blocks()
        .get(1)
        .ok_or("should have preview block")?;
    assert!(
        matches!(preview_block.source, BlockSource::ToolDisplay { .. }),
        "the preview must be a ToolDisplay block"
    );
    assert_eq!(
        preview_block.fill,
        Fill::Code,
        "a diff preview block must carry Fill::Code"
    );
    assert!(
        tool_display_lines(preview_block).contains("+new content"),
        "the preview block must carry the display content, got: {}",
        tool_display_lines(preview_block)
    );
    Ok(())
}

/// The request context carries the exact call key, so a decorated display name
/// in `tool` cannot break the match. This is the production shape: `tool` is
/// `edit(path=..., operation={...})` while the pending call key is
/// `edit\n{...}`.
#[test]
fn permission_requested_with_decorated_tool_name_attaches_preview_via_tool_key() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    let arguments =
        r#"{"path":"foo.rs","mode":"apply","operation":{"type":"create","content":"hi\n"}}"#;
    reduce_tool(&mut state, started("edit", arguments));

    let context = nu_agent_core::protocol::event::PermissionRequestContext {
        tool: "edit(mode=apply, operation={...}, path=foo.rs)".to_string(),
        tool_key: format!("edit\n{arguments}"),
        source: "builtin".to_string(),
        mode: Some("apply".to_string()),
        matched_rule_identity: "tool:edit".to_string(),
        scope: "tool".to_string(),
        target_field: None,
        pattern: "edit".to_string(),
        summary: "→ {...}".to_string(),
        pre_authorize_display: Some(ToolDisplay {
            title: "edit foo.rs".to_string(),
            sections: vec![ToolDisplaySection {
                label: "changes".to_string(),
                kind: ContentKind::Diff {
                    language: "diff".to_string(),
                },
                content: "+hi".to_string(),
                stats: None,
            }],
        }),
    };

    // -- Exec
    apply_permission_request_display(&mut state, &context);

    // -- Check
    assert_eq!(
        state.transcript.len(),
        2,
        "preview must push its own ToolDisplay block"
    );
    let tool_block = state
        .transcript
        .blocks()
        .first()
        .ok_or("should have tool block")?;
    assert!(
        matches!(tool_block.source, BlockSource::Tool { preview: None, .. }),
        "the Tool block must keep preview None"
    );
    let preview_block = state
        .transcript
        .blocks()
        .get(1)
        .ok_or("should have preview block")?;
    assert!(
        tool_display_lines(preview_block).contains("+hi"),
        "decorated tool name must still attach the preview via tool_key, got: {}",
        tool_display_lines(preview_block)
    );
    Ok(())
}

/// A request whose `tool_key` matches no pending call must leave the
/// transcript untouched — no preview attached to an unrelated call.
#[test]
fn permission_requested_with_unmatched_tool_key_leaves_transcript_unchanged() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    reduce_tool(&mut state, started("edit", r#"{"path":"foo.rs"}"#));

    let context = nu_agent_core::protocol::event::PermissionRequestContext {
        tool: "edit(path=other.rs)".to_string(),
        tool_key: "edit\n{\"path\":\"other.rs\"}".to_string(),
        source: "builtin".to_string(),
        mode: Some("apply".to_string()),
        matched_rule_identity: "tool:edit".to_string(),
        scope: "tool".to_string(),
        target_field: None,
        pattern: "edit".to_string(),
        summary: "→ {...}".to_string(),
        pre_authorize_display: Some(ToolDisplay {
            title: "edit other.rs".to_string(),
            sections: vec![],
        }),
    };

    // -- Exec
    apply_permission_request_display(&mut state, &context);

    // -- Check
    assert_eq!(state.transcript.len(), 1);
    let block = state
        .transcript
        .blocks()
        .first()
        .ok_or("should have tool block")?;
    let BlockSource::Tool { preview, .. } = &block.source else {
        panic!("expected Tool block");
    };
    assert!(
        preview.is_none(),
        "an unmatched tool_key must not attach a preview"
    );
    Ok(())
}

/// Non-duplication is no longer this layer's job: `HookChain::on_tool_result`
/// (nu-agent-core) omits `display` from the `Completed` event whenever a
/// pre-authorize preview was already shown, so the reducer here never
/// receives a duplicate to begin with. This test documents that contract at
/// the TUI boundary: when the source correctly sends `display: None` after a
/// preview, the transcript shows the preview exactly once.
#[test]
fn tool_end_after_previewed_permission_with_source_suppressed_display_shows_once() {
    let mut state = AppState::default();

    reduce_tool(&mut state, started("edit", r#"{"file":"bar.rs"}"#));

    let context = nu_agent_core::protocol::event::PermissionRequestContext {
        tool: "edit(file=bar.rs)".to_string(),
        tool_key: "edit\n{\"file\":\"bar.rs\"}".to_string(),
        source: "closure".to_string(),
        mode: Some("apply".to_string()),
        matched_rule_identity: "tool:edit".to_string(),
        scope: "tool".to_string(),
        target_field: None,
        pattern: "edit".to_string(),
        summary: "→ {...}".to_string(),
        pre_authorize_display: Some(ToolDisplay {
            title: "edit bar.rs".to_string(),
            sections: vec![ToolDisplaySection {
                label: "changes".to_string(),
                kind: ContentKind::Diff {
                    language: "diff".to_string(),
                },
                content: "+new content".to_string(),
                stats: None,
            }],
        }),
    };
    apply_permission_request_display(&mut state, &context);

    // The source (chain.rs's suppress_previewed_display) already omitted the
    // display here — that's the actual non-duplication contract.
    reduce_tool(
        &mut state,
        ToolEvent::Completed {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"file":"bar.rs"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: None,
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

    assert_eq!(
        lines
            .iter()
            .filter(|line| line.contains("+new content"))
            .count(),
        1,
        "the preview must appear exactly once, got: {lines:?}"
    );
}

/// The TUI reducer itself does not deduplicate: if a `Completed` event were
/// ever to carry a display after a preview was already shown (a source bug),
/// the transcript would show it twice. This isn't desired behavior — it's a
/// regression guard documenting that the non-duplication guarantee lives
/// entirely in `nu-agent-core`'s `suppress_previewed_display`
/// (`hook/chain.rs`), not here.
#[test]
fn tool_end_renders_whatever_display_it_is_given_no_local_dedup() {
    let mut state = AppState::default();

    reduce_tool(&mut state, started("edit", r#"{"file":"bar.rs"}"#));

    let context = nu_agent_core::protocol::event::PermissionRequestContext {
        tool: "edit(file=bar.rs)".to_string(),
        tool_key: "edit\n{\"file\":\"bar.rs\"}".to_string(),
        source: "closure".to_string(),
        mode: Some("apply".to_string()),
        matched_rule_identity: "tool:edit".to_string(),
        scope: "tool".to_string(),
        target_field: None,
        pattern: "edit".to_string(),
        summary: "→ {...}".to_string(),
        pre_authorize_display: Some(ToolDisplay {
            title: "edit bar.rs".to_string(),
            sections: vec![ToolDisplaySection {
                label: "changes".to_string(),
                kind: ContentKind::Diff {
                    language: "diff".to_string(),
                },
                content: "+new content".to_string(),
                stats: None,
            }],
        }),
    };
    apply_permission_request_display(&mut state, &context);

    reduce_tool(
        &mut state,
        ToolEvent::Completed {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"file":"bar.rs"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
                title: "edit bar.rs".to_string(),
                sections: vec![ToolDisplaySection {
                    label: "changes".to_string(),
                    kind: ContentKind::Diff {
                        language: "diff".to_string(),
                    },
                    content: "+new content".to_string(),
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

    assert_eq!(
        lines
            .iter()
            .filter(|line| line.contains("+new content"))
            .count(),
        2,
        "the reducer has no dedup of its own — a source that (incorrectly) \
         resends the display after a preview will show it twice; got: {lines:?}"
    );
}

#[test]
fn tool_end_without_prior_permission_pushes_display_normally() {
    let mut state = AppState::default();

    reduce_tool(&mut state, started("edit", r#"{"path":"bar.rs"}"#));

    reduce_tool(
        &mut state,
        ToolEvent::Completed {
            name: "edit".to_string(),
            source: "mcp".to_string(),
            arguments: r#"{"path":"bar.rs"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
                title: "edit bar.rs".to_string(),
                sections: vec![ToolDisplaySection {
                    label: "changes".to_string(),
                    kind: ContentKind::Diff {
                        language: "diff".to_string(),
                    },
                    content: "+new content".to_string(),
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
        !lines.iter().any(|line| line.contains("changes (diff)")),
        "the call line already shows the path, so the section label is redundant"
    );
    assert!(
        lines.iter().any(|line| line.contains("+new content")),
        "Expected to find '+new content' in transcript"
    );
}

#[test]
fn permission_requested_without_display_does_not_add_transcript_entries() {
    let mut state = AppState::default();

    reduce_tool(&mut state, started("nu", r#"{"command":"ls"}"#));

    let len_after_start = state.transcript.len();

    let context = nu_agent_core::protocol::event::PermissionRequestContext {
        tool: "nu(command=ls)".to_string(),
        tool_key: "nu\n{\"command\":\"ls\"}".to_string(),
        source: "mcp".to_string(),
        mode: None,
        matched_rule_identity: "tool:nu".to_string(),
        scope: "tool".to_string(),
        target_field: None,
        pattern: "nu".to_string(),
        summary: r#"→ {"command":"ls"}"#.to_string(),
        pre_authorize_display: None,
    };
    state
        .permission
        .reduce_permission_event(nu_agent_core::bus::PermissionEvent::Requested {
            request_id: "req-1".to_string(),
            context: Box::new(context),
        });

    assert_eq!(
        state.transcript.len(),
        len_after_start,
        "PermissionRequested without display should not add transcript entries"
    );
}
