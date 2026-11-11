//! The snapshot projection: the record log a persisted session replays as.
//!
//! The projection is the one bridge between the store's snapshot list and the
//! stream's vocabulary. A tool row's stored result is the call's data: it rides
//! the completion as the payload a drawer derives from and as the text the model
//! received.

use crate::protocol::contracts::{UiMessageSnapshot, project_snapshots};
use crate::protocol::events::{SessionRecord, ToolRecord, TurnRecord};
use crate::session::tool_status::ToolStatus;

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// -- Test Support

/// One persisted tool row: the call as data, the result the store recorded, and
/// the verdict the store recorded beside it.
fn tool_row(
    name: &str,
    arguments: &str,
    result: Option<&str>,
    success: Option<bool>,
) -> UiMessageSnapshot {
    UiMessageSnapshot::new("tool", String::new())
        .with_tool_name(name.to_string())
        .with_tool_details(
            Some(arguments.to_string()),
            result.map(str::to_string),
            success,
        )
}

/// WHEN a tool snapshot carries a stored result, THE projection SHALL carry it
/// into the completion's `ToolOutcome` as both payload and text; a result-less
/// call SHALL keep the empty outcome and its stored verdict; a display row SHALL
/// contribute nothing; and each replayed turn SHALL close with exactly one
/// `TurnRecord::Completed`.
#[test]
fn test_protocol_projection_tool_row_carries_its_result_into_the_outcome() -> Result<()> {
    // -- Setup & Fixtures: two replayed turns, one call each, plus a display row.
    let edit_result = r#"{"path":"a.rs","diff":"--- a\n+++ b\n"}"#;
    let messages = vec![
        UiMessageSnapshot::new("user", "first question"),
        tool_row("edit", r#"{"path":"a.rs"}"#, Some(edit_result), Some(true)),
        UiMessageSnapshot::new("tool_display", "ignored"),
        UiMessageSnapshot::new("user", "second question"),
        tool_row("nu", r#"{"command":"ls"}"#, None, Some(false)),
    ];

    // -- Exec
    let (records, _total) = project_snapshots(messages);

    // -- Check: one start and one completion per call; the display row names no
    // call and contributes no record.
    let outcomes: Vec<&agnt_kernel::ToolOutcome> = records
        .iter()
        .filter_map(|record| match record {
            SessionRecord::Tool(ToolRecord::Completed { outcome, .. }) => Some(outcome),
            _ => None,
        })
        .collect();
    assert_eq!(outcomes.len(), 2, "one completion per replayed call");

    // -- Check: the stored result rides the outcome as the payload the drawer
    // derives from and as the text the model received.
    let edit = outcomes.first().ok_or("should have the edit outcome")?;
    assert_eq!(edit.payload.as_deref(), Some(edit_result));
    assert_eq!(edit.text, edit_result);
    assert_eq!(edit.status, ToolStatus::Success);

    // -- Check: a result-less call keeps today's empty outcome and its stored
    // verdict.
    let nu = outcomes.get(1).ok_or("should have the nu outcome")?;
    assert_eq!(nu.payload, None, "no result, no payload");
    assert_eq!(nu.text, "", "no result, no text");
    assert_eq!(nu.status, ToolStatus::Failed, "the stored verdict");

    // -- Check: the boundary evidence — one at-rest marker per replayed turn.
    let boundaries = records
        .iter()
        .filter(|record| matches!(record, SessionRecord::Turn(TurnRecord::Completed { .. })))
        .count();
    assert_eq!(boundaries, 2, "one boundary per replayed turn");
    Ok(())
}
