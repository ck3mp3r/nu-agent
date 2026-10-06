use super::*;

// ---------------------------------------------------------------------------
// Tool preview dispatch
// ---------------------------------------------------------------------------

#[test]
fn tool_preview_dispatch_pushes_display_block_after_pending_tool() -> Result<()> {
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
                title: "nu".to_string(),
                sections: vec![ToolDisplaySection {
                    label: "nu".to_string(),
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
    assert_eq!(
        state.transcript.len(),
        2,
        "ToolPreview must push one ToolDisplay block after the Tool block"
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
        matches!(preview_block.source, BlockSource::ToolDisplay { .. }),
        "the preview must be a ToolDisplay block"
    );
    assert!(
        tool_display_lines(preview_block).contains("ls"),
        "the preview block must carry the display content"
    );
    Ok(())
}

#[test]
fn tool_preview_dispatch_with_unmatched_key_pushes_no_block() {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    dispatch_ui_event(
        &mut state,
        UiEvent::ToolStarted {
            name: "nu".to_string(),
            source: "builtin".to_string(),
            arguments: r#"{"command":"ls"}"#.to_string(),
            call_line: CallLine::from_json_summary(r#"{"command":"ls"}"#),
        },
    );
    let len_after_start = state.transcript.len();

    // -- Exec
    dispatch_ui_event(
        &mut state,
        UiEvent::ToolPreview {
            tool_key: "nu\n{\"command\":\"other\"}".to_string(),
            display: ToolDisplay {
                title: "nu".to_string(),
                sections: vec![],
            },
        },
    );

    // -- Check
    assert_eq!(
        state.transcript.len(),
        len_after_start,
        "an unmatched tool_key must push no block"
    );
}
