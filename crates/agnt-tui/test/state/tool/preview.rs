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
