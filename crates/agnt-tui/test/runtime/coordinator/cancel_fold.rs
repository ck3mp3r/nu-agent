//! The ESC-mid-run fold: a busy tool row leaves `InProgress` when its cancel
//! settles.
//!
//! The spinner runs while any tool row is in progress, so a call whose cancel
//! settles as cancelled is what stops it. This pins the fold side of the
//! sequence the executor publishes for a stop that lands mid-run.

use super::*;

use crate::test_support::{tool_completed, tool_started};
use agnt_render::transcript::entry::TranscriptEntry;
use agnt_render::transcript::renderer::ItemStatus;
use agnt_runtime::bus::LifecycleEvent;
use agnt_runtime::protocol::events::{
    InferenceId, InferenceRecord, SessionRecord, StopCause, ToolCall, ToolErrorKind, ToolOutcome,
    ToolStatus, TurnRecord,
};
use agnt_runtime::tools::handler::ToolSource;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// WHEN the coordinator folds the stop's records for a running call, THE tool
/// row SHALL leave `InProgress` as `Cancelled` and the frame SHALL stop
/// animating with no other active work.
#[test]
fn test_runtime_coordinator_cancelled_tool_row_leaves_in_progress() -> Result<()> {
    // -- Setup & Fixtures: one dispatched call, and the cancel that settles it.
    let mut coordinator = RuntimeCoordinator::new(80, 24, None);
    let call = ToolCall::new("grep", ToolSource::Builtin, "{\"pattern\":\"x\"}");
    let cancelled = ToolOutcome {
        status: ToolStatus::Cancelled,
        payload: None,
        text: String::new(),
        message: None,
        kind: Some(ToolErrorKind::Cancelled),
    };

    // -- Exec: the completion call opens and dispatches the tool, then the
    // stop's sequence settles the call and releases the turn.
    let opened_changed = coordinator.reduce_record_batch(vec![
        SessionRecord::Inference(InferenceRecord::Started {
            inference: InferenceId::mint(),
        }),
        tool_started(call.clone()),
    ]);
    let row_while_running = tool_row_status(&coordinator);
    let animating_while_running = coordinator.has_active_animation();

    let settled_changed = coordinator.reduce_record_batch(vec![
        tool_completed(call, cancelled),
        SessionRecord::Inference(InferenceRecord::Stopped {
            cause: StopCause::UserCancelled,
            reason: "Turn stopped".to_string(),
        }),
        SessionRecord::Turn(TurnRecord::Completed { duration_ms: 1 }),
    ]);
    coordinator.reduce_lifecycle(&LifecycleEvent::TurnCompleted {
        task_id: None,
        duration_ms: 1,
    });
    let row_after_settle = tool_row_status(&coordinator);
    let animating_after_settle = coordinator.has_active_animation();

    // -- Check
    assert!(
        opened_changed,
        "the call's open and its dispatch must reach the transcript"
    );
    assert_eq!(
        row_while_running,
        Some(ItemStatus::InProgress),
        "a running call must hold its row in progress"
    );
    assert!(
        animating_while_running,
        "a running call must animate the frame while the turn is busy"
    );
    assert!(settled_changed, "the settle must reach the transcript");
    assert_eq!(
        row_after_settle,
        Some(ItemStatus::Cancelled),
        "the cancel must leave the row out of in-progress"
    );
    assert!(
        !animating_after_settle,
        "a settled cancel must stop the spinner"
    );
    Ok(())
}

/// WHEN the coordinator folds a not-run sibling pair, THE row SHALL settle
/// `Cancelled` and the frame SHALL carry no active work once the turn releases.
///
/// The pair is the executor's flush for a call the stop left undispatched: the
/// completion carries the cancelled kind and the not-run message, and no result
/// text, so the row keeps its call line and settles.
#[test]
fn test_runtime_coordinator_not_run_sibling_pair_settles_the_row_as_cancelled() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(80, 24, None);
    let call = ToolCall::new("edit", ToolSource::Closure, "{\"path\":\"notes.md\"}");
    let denied = ToolOutcome {
        status: ToolStatus::Cancelled,
        payload: None,
        text: String::new(),
        message: Some(agnt_runtime::hook::chain::CANCELLED_DISPATCH_MESSAGE.to_string()),
        kind: Some(ToolErrorKind::Cancelled),
    };

    // -- Exec
    coordinator.reduce_record_batch(vec![tool_started(call.clone())]);
    coordinator.reduce_record_batch(vec![tool_completed(call, denied)]);
    coordinator.reduce_lifecycle(&LifecycleEvent::TurnCompleted {
        task_id: None,
        duration_ms: 1,
    });

    // -- Check
    assert_eq!(
        tool_row_status(&coordinator),
        Some(ItemStatus::Cancelled),
        "the not-run sibling's row must settle cancelled"
    );
    assert!(
        !coordinator.has_active_animation(),
        "a settled not-run pair must stop the spinner"
    );
    assert_eq!(
        coordinator.state.transcript.entries().len(),
        1,
        "the pair settles one row, never a second block"
    );
    Ok(())
}

// region:    --- Test Support

/// The status of the transcript's one tool row, when it holds one.
fn tool_row_status(coordinator: &RuntimeCoordinator) -> Option<ItemStatus> {
    coordinator
        .state
        .transcript
        .entries()
        .iter()
        .find_map(|entry| match entry {
            TranscriptEntry::Tool(tool) => Some(tool.status),
            _ => None,
        })
}

// endregion: --- Test Support
