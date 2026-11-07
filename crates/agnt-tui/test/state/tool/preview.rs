use super::*;

// ---------------------------------------------------------------------------
// Tool preview dispatch
// ---------------------------------------------------------------------------

/// `PreviewRequested` sets the matching tool entry's preview in place and reports
/// handled; an unmatched key leaves the entry untouched. The card itself is
/// derived from the call, so the event carries no content for `nu`.
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
            preview: None,
        },
    );

    // -- Check
    assert!(handled, "ToolPreview must report a handled event");
    assert_eq!(state.transcript.len(), 1, "the preview lives on the entry");
    let tool = tool_entry_at(&state, 0)?;
    let preview = tool.preview.as_ref().ok_or("should have preview")?;
    assert!(display_lines(preview).contains("ls"));

    // -- Exec & Check: an unmatched tool_key leaves the entry unchanged.
    let len_before = state.transcript.len();
    reduce_ui(
        &mut state,
        SystemEvent::PreviewRequested {
            call: tool_call("nu", "{\"command\":\"other\"}"),
            preview: None,
        },
    );
    assert_eq!(state.transcript.len(), len_before);
    Ok(())
}

/// A cancelled call's live sequence — Started, PreviewRequested, Completed with
/// the cancelled error kind — leaves ONE tool entry carrying its preview and the
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
            preview: None,
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
                error_kind: Some("cancelled".to_string()),
                message: Some(agnt_runtime::hook::chain::CANCELLED_DISPATCH_MESSAGE.to_string()),
            },
        },
    );

    // -- Check
    assert_eq!(state.transcript.len(), 1);
    let tool = tool_entry_at(&state, 0)?;
    assert_eq!(tool.status, ItemStatus::Cancelled);
    let preview = tool.preview.as_ref().ok_or("should have preview")?;
    assert!(display_lines(preview).contains("ls"));
    Ok(())
}
