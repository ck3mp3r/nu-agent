use super::*;

// ---------------------------------------------------------------------------
// Tool preview dispatch
// ---------------------------------------------------------------------------

/// `ToolPreview` pushes one ToolDisplay block after the matching Tool block and
/// reports handled; an unmatched key pushes nothing.
#[test]
fn tool_preview_dispatch_pushes_matching_block_and_ignores_unmatched_key() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    let arguments = r#"{"command":"ls"}"#;
    dispatch_ui_event(
        &mut state,
        UiEvent::ToolStarted {
            name: "nu".to_string(),
            source: "builtin".to_string(),
            arguments: arguments.to_string(),
            call_line: CallLine::from_json_summary(arguments),
        },
    );

    // -- Exec
    let handled = dispatch_ui_event(
        &mut state,
        UiEvent::ToolPreview {
            tool_key: format!("nu\n{arguments}"),
            display: ToolDisplay {
                title: Some("nu".to_string()),
                sections: vec![ToolDisplaySection {
                    label: Some("nu".to_string()),
                    kind: ContentKind::Code {
                        language: "nu".to_string(),
                    },
                    content: "ls".to_string(),
                    stats: None,
                }],
            },
        },
    );

    // -- Check
    assert!(handled, "ToolPreview must report a handled event");
    assert_eq!(state.transcript.len(), 2);
    let tool_block = state
        .transcript
        .blocks()
        .first()
        .ok_or("should have tool block")?;
    assert!(matches!(
        tool_block.source,
        BlockSource::Tool { preview: None, .. }
    ));
    let preview_block = state
        .transcript
        .blocks()
        .get(1)
        .ok_or("should have preview block")?;
    assert!(matches!(
        preview_block.source,
        BlockSource::ToolDisplay { .. }
    ));
    assert!(tool_display_lines(preview_block).contains("ls"));

    // -- Exec & Check: an unmatched tool_key pushes no block.
    let len_before = state.transcript.len();
    dispatch_ui_event(
        &mut state,
        UiEvent::ToolPreview {
            tool_key: "nu\n{\"command\":\"other\"}".to_string(),
            display: ToolDisplay {
                title: Some("nu".to_string()),
                sections: vec![],
            },
        },
    );
    assert_eq!(state.transcript.len(), len_before);
    Ok(())
}

/// A cancelled call's live sequence — Started, ToolPreview, Completed with the
/// cancelled error kind — leaves the tool block (status Cancelled) followed by
/// its preview block. The preview anchors to the call while it is still
/// `InProgress`, so the two-block shape matches rehydration.
#[test]
fn tool_preview_between_started_and_cancelled_completed_keeps_two_block_shape() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    let arguments = r#"{"command":"ls"}"#;
    dispatch_ui_event(
        &mut state,
        UiEvent::ToolStarted {
            name: "nu".to_string(),
            source: "builtin".to_string(),
            arguments: arguments.to_string(),
            call_line: CallLine::from_json_summary(arguments),
        },
    );

    // -- Exec
    dispatch_ui_event(
        &mut state,
        UiEvent::ToolPreview {
            tool_key: format!("nu\n{arguments}"),
            display: ToolDisplay {
                title: None,
                sections: vec![ToolDisplaySection {
                    label: None,
                    kind: ContentKind::Code {
                        language: "nu".to_string(),
                    },
                    content: "ls".to_string(),
                    stats: None,
                }],
            },
        },
    );
    dispatch_ui_event(
        &mut state,
        UiEvent::ToolCompleted {
            name: "nu".to_string(),
            source: "builtin".to_string(),
            arguments: arguments.to_string(),
            success: false,
            result: String::new(),
            display: None,
            error_kind: Some("cancelled".to_string()),
            message: Some(agnt_core::hook::chain::CANCELLED_DISPATCH_MESSAGE.to_string()),
        },
    );

    // -- Check
    assert_eq!(state.transcript.len(), 2);
    let tool_block = state
        .transcript
        .blocks()
        .first()
        .ok_or("should have tool block")?;
    assert!(matches!(
        tool_block.source,
        BlockSource::Tool { preview: None, .. }
    ));
    assert_eq!(tool_block.status, Some(ItemStatus::Cancelled));
    let preview_block = state
        .transcript
        .blocks()
        .get(1)
        .ok_or("should have preview block")?;
    assert!(matches!(
        preview_block.source,
        BlockSource::ToolDisplay { .. }
    ));
    assert!(tool_display_lines(preview_block).contains("ls"));
    Ok(())
}
