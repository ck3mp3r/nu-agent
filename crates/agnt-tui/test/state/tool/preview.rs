use super::*;

use crate::test_support::{tool_completed, tool_started};

// ---------------------------------------------------------------------------
// Tool preview dispatch
// ---------------------------------------------------------------------------

/// A permission request's card is derived from the call the request names: the
/// matching pending entry gains the preview in place, and a request naming
/// another call leaves the entry untouched.
#[test]
fn tool_preview_dispatch_sets_preview_in_place_and_ignores_unmatched_key() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    let arguments = r#"{"command":"ls"}"#;
    reduce_ui(&mut state, tool_started(tool_call("nu", arguments)));

    // -- Exec
    apply_permission_request_display(
        &mut state,
        &permission_requested("nu(command=ls)", &format!("nu\n{arguments}"), None),
    );

    // -- Check
    assert_eq!(state.transcript.len(), 1, "the preview lives on the entry");
    let tool = tool_entry_at(&state, 0)?;
    let preview = tool.preview.as_ref().ok_or("should have preview")?;
    assert!(display_lines(preview).contains("ls"));

    // -- Exec & Check: an unmatched tool_key leaves the entry unchanged.
    let mut other = AppState::default();
    reduce_ui(&mut other, tool_started(tool_call("nu", arguments)));
    apply_permission_request_display(
        &mut other,
        &permission_requested("nu(command=other)", "nu\n{\"command\":\"other\"}", None),
    );
    assert!(tool_entry_at(&other, 0)?.preview.is_none());
    Ok(())
}

/// A cancelled call's live sequence — Started, the permission card, Completed
/// with the cancelled error kind — leaves ONE tool entry carrying its preview
/// and the Cancelled status.
#[test]
fn tool_preview_between_started_and_cancelled_completed_keeps_one_entry() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    let arguments = r#"{"command":"ls"}"#;
    reduce_ui(&mut state, tool_started(tool_call("nu", arguments)));

    // -- Exec
    apply_permission_request_display(
        &mut state,
        &permission_requested("nu(command=ls)", &format!("nu\n{arguments}"), None),
    );
    reduce_ui(
        &mut state,
        tool_completed(
            tool_call("nu", arguments),
            ToolOutcome {
                status: ToolStatus::Cancelled,
                payload: None,
                text: String::new(),
                message: Some(agnt_runtime::hook::chain::CANCELLED_DISPATCH_MESSAGE.to_string()),
                kind: Some(ToolErrorKind::Cancelled),
            },
        ),
    );

    // -- Check
    assert_eq!(state.transcript.len(), 1);
    let tool = tool_entry_at(&state, 0)?;
    assert_eq!(tool.status, ItemStatus::Cancelled);
    let preview = tool.preview.as_ref().ok_or("should have preview")?;
    assert!(display_lines(preview).contains("ls"));
    Ok(())
}
