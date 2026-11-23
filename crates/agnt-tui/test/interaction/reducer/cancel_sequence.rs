//! The ESC-mid-run record sequence folded through the reducer.
//!
//! The executor publishes the same six records main did for a stop that lands
//! mid-run. These tests fold them into the surface: the running call's row
//! settles cancelled, the turn's generic notice renders, and no warning does.

use super::*;

use crate::runtime::coordinator::RuntimeCoordinator;
use crate::state::StatusMessageKind;
use agnt_render::transcript::renderer::ItemStatus;
use agnt_runtime::protocol::events::{
    InferenceRecord, SessionRecord, StopCause, ToolCall, ToolErrorKind, ToolOutcome, ToolStatus,
};
use agnt_runtime::tools::handler::ToolSource;

/// The stop records an ESC-mid-run turn publishes, in main's order: the hook's
/// stop reason, then the executor's generic turn notice.
fn stop_records() -> Vec<SessionRecord> {
    vec![
        SessionRecord::Inference(InferenceRecord::Stopped {
            cause: StopCause::UserCancelled,
            reason: agnt_runtime::hook::chain::CANCELLED_STOP_REASON.to_string(),
        }),
        SessionRecord::Inference(InferenceRecord::Stopped {
            cause: StopCause::UserCancelled,
            reason: "Turn stopped".to_string(),
        }),
    ]
}

/// The call the stop interrupted, settled the way the executor settles it.
fn cancelled_call() -> (ToolCall, ToolOutcome) {
    (
        ToolCall::new("grep", ToolSource::Builtin, "{\"pattern\":\"x\"}"),
        ToolOutcome {
            status: ToolStatus::Cancelled,
            payload: None,
            text: "tool call cancelled".to_string(),
            message: None,
            kind: Some(ToolErrorKind::Cancelled),
        },
    )
}

/// Fold the sequence an ESC mid-run publishes: the call opens, the stop arrives
/// as the hook and the executor narrate it, then the spine releases the turn.
fn fold_esc_sequence(state: &mut AppState) {
    let _ = reduce(state, RenderInput::Record(&inference_started()), None);
    let (call, outcome) = cancelled_call();
    let started = tool_started(call.clone());
    let _ = reduce(state, RenderInput::Record(&started), None);
    let completed = tool_completed(call, outcome);
    let _ = reduce(state, RenderInput::Record(&completed), None);
    for record in stop_records() {
        let _ = reduce(state, RenderInput::Record(&record), None);
    }
    let _ = fold_turn_completed(state);
}

/// The status of the transcript's one tool row, when it holds one.
fn tool_row_status(state: &AppState) -> Option<ItemStatus> {
    state
        .transcript
        .entries()
        .iter()
        .find_map(|entry| match entry {
            TranscriptEntry::Tool(tool) => Some(tool.status),
            _ => None,
        })
}

/// WHEN the TUI folds the ESC-mid-run sequence, THE running call's row SHALL
/// leave `InProgress` as `Cancelled`.
#[test]
fn test_reducer_esc_mid_run_settles_the_running_row_as_cancelled() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    let (call, outcome) = cancelled_call();
    let started = tool_started(call.clone());
    let completed = tool_completed(call, outcome);
    let _ = reduce(&mut state, RenderInput::Record(&inference_started()), None);
    let _ = reduce(&mut state, RenderInput::Record(&started), None);

    // -- Exec
    let row_while_running = tool_row_status(&state);
    let _ = reduce(&mut state, RenderInput::Record(&completed), None);
    let row_after_settle = tool_row_status(&state);

    // -- Check
    assert_eq!(
        row_while_running,
        Some(ItemStatus::InProgress),
        "a dispatched call must hold its row in progress"
    );
    assert_eq!(
        row_after_settle,
        Some(ItemStatus::Cancelled),
        "the cancel must leave the row out of in-progress"
    );
    Ok(())
}

/// WHEN the coordinator folds the ESC-mid-run sequence, THE frame SHALL stop
/// animating and the turn's generic stop notice SHALL stay the status line: the
/// notice is the surface's row for the stop the executor narrated.
#[test]
fn test_reducer_esc_mid_run_stops_the_animation_after_the_notice() -> Result<()> {
    // -- Setup & Fixtures: the running call opens the turn.
    let mut coord = RuntimeCoordinator::new(80, 24, None);
    let (call, outcome) = cancelled_call();
    let _ = reduce(
        &mut coord.state,
        RenderInput::Record(&inference_started()),
        None,
    );
    let started = tool_started(call.clone());
    let _ = reduce(&mut coord.state, RenderInput::Record(&started), None);
    let animating_while_running = coord.has_active_animation();

    // -- Exec: the stop settles the call, narrates the notice, and releases.
    let completed = tool_completed(call, outcome);
    let _ = reduce(&mut coord.state, RenderInput::Record(&completed), None);
    for record in stop_records() {
        let _ = reduce(&mut coord.state, RenderInput::Record(&record), None);
    }
    let _ = fold_turn_completed(&mut coord.state);

    // -- Check: the notice is on screen and nothing animates.
    assert!(
        animating_while_running,
        "a dispatched call must animate the frame"
    );
    assert_eq!(
        coord.state.status.message.status_line(),
        "Turn stopped",
        "the turn's generic stop notice must be the status line"
    );
    assert!(
        !coord.has_active_animation(),
        "the settled stop must stop the animation"
    );
    Ok(())
}

/// WHEN the TUI folds the ESC-mid-run sequence, THE turn's generic stop notice
/// SHALL render as a notice that survives the release, and NO warning SHALL
/// render beside it.
#[test]
fn test_reducer_esc_mid_run_renders_the_generic_stop_notice_without_a_warning() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec
    fold_esc_sequence(&mut state);

    // -- Check
    assert_eq!(
        state.status.message.status_line(),
        "Turn stopped",
        "the executor's generic notice must be the status line"
    );
    assert_eq!(
        state.status.message.kind(),
        StatusMessageKind::Notice,
        "the stop notice must be a notice, never a warning"
    );
    assert_eq!(
        state.phase,
        UiPhase::Idle,
        "the turn's release must return the surface to idle"
    );
    Ok(())
}
