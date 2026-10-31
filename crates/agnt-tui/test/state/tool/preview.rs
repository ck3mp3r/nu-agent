use super::*;

// ---------------------------------------------------------------------------
// Tool preview dispatch
// ---------------------------------------------------------------------------

/// `ToolPreview` sets the matching tool entry's preview in place and reports
/// handled; an unmatched key leaves the entry untouched.
#[test]
fn tool_preview_dispatch_sets_preview_in_place_and_ignores_unmatched_key() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    let arguments = r#"{"command":"ls"}"#;
    reduce_ui(
        &mut state,
        SystemEvent::ToolStarted {
            call: tool_call("nu", arguments),
        },
    );

    // -- Exec
    let handled = reduce_ui(
        &mut state,
        SystemEvent::PreviewRequested {
            call: tool_call("nu", arguments),
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
    assert_eq!(state.transcript.len(), 1, "the preview lives on the entry");
    let entry = state
        .transcript
        .entries()
        .first()
        .ok_or("should have tool entry")?;
    let TranscriptEntry::Tool(tool) = entry else {
        return Err("expected Tool entry".into());
    };
    let preview = tool.preview.as_ref().ok_or("should have preview")?;
    assert!(display_lines(preview).contains("ls"));

    // -- Exec & Check: an unmatched tool_key leaves the entry unchanged.
    let len_before = state.transcript.len();
    reduce_ui(
        &mut state,
        SystemEvent::PreviewRequested {
            call: tool_call("nu", "{\"command\":\"other\"}"),
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
/// cancelled error kind — leaves ONE tool entry carrying its preview and the
/// Cancelled status.
#[test]
fn tool_preview_between_started_and_cancelled_completed_keeps_one_entry() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    let arguments = r#"{"command":"ls"}"#;
    reduce_ui(
        &mut state,
        SystemEvent::ToolStarted {
            call: tool_call("nu", arguments),
        },
    );

    // -- Exec
    reduce_ui(
        &mut state,
        SystemEvent::PreviewRequested {
            call: tool_call("nu", arguments),
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
    reduce_ui(
        &mut state,
        SystemEvent::ToolCompleted {
            call: tool_call("nu", arguments),
            outcome: ToolOutcome {
                success: false,
                status: ToolStatus::Cancelled,
                result: String::new(),
                display: None,
                error_kind: Some("cancelled".to_string()),
                message: Some(agnt_core::hook::chain::CANCELLED_DISPATCH_MESSAGE.to_string()),
            },
        },
    );

    // -- Check
    assert_eq!(state.transcript.len(), 1);
    let entry = state
        .transcript
        .entries()
        .first()
        .ok_or("should have tool entry")?;
    let TranscriptEntry::Tool(tool) = entry else {
        return Err("expected Tool entry".into());
    };
    assert_eq!(tool.status, ItemStatus::Cancelled);
    let preview = tool.preview.as_ref().ok_or("should have preview")?;
    assert!(display_lines(preview).contains("ls"));
    Ok(())
}
