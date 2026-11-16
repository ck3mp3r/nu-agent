//! Hydration replay: the snapshot projection and the replay determinism.
//!
//! The projection tests pin the snapshot to event mapping. The determinism test
//! is the hydration contract: the state a replay produces equals the state the
//! same event log produces when the reducer folds it live.
//!
//! Split by topic: `projection.rs` holds the snapshot-to-record mapping,
//! `replay.rs` holds the replay lane and its determinism contract. The fixtures
//! stay here, where `super::*` reaches them from both.
use super::*;

use crate::interaction::cancel::CancelController;
use crate::interaction::reducer::{RenderInput, reduce};
use crate::test_support::entry_text;
use agnt_render::transcript::entry::{MessageEntry, TranscriptEntry};
use agnt_render::transcript::ir::{Display, MessageRole};
use agnt_render::transcript::renderer::ItemStatus;
use agnt_runtime::protocol::contracts::{
    UiMessageSnapshot, UiMessageUsageSnapshot, project_snapshots,
};
use agnt_runtime::protocol::events::{InferenceRecord, SessionRecord, ToolRecord};
use agnt_runtime::session::tool_status::ToolStatus;

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// One display flattened to the text a backend draws, line by line.
fn display_text(display: &Display) -> String {
    display
        .project_lines()
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// region:    --- Fixtures

fn usage(input_tokens: u64, output_tokens: u64, total_tokens: u64) -> UiMessageUsageSnapshot {
    UiMessageUsageSnapshot {
        input_tokens: Some(input_tokens),
        output_tokens: Some(output_tokens),
        total_tokens: Some(total_tokens),
    }
}

fn assistant(text: &str, usage: Option<UiMessageUsageSnapshot>) -> UiMessageSnapshot {
    let mut snapshot = UiMessageSnapshot::new("assistant", text);
    snapshot.usage = usage;
    snapshot
}

/// One persisted tool row: the call it names, the result the model received,
/// and the verdict the store recorded. The content IS the tool's data — the
/// surface derives the body from it, live and on replay alike.
fn tool(name: &str, arguments: &str, result: &str, success: Option<bool>) -> UiMessageSnapshot {
    UiMessageSnapshot::new("tool", result.to_string())
        .with_tool_name(name.to_string())
        .with_tool_details(
            Some(arguments.to_string()),
            Some(result.to_string()),
            success,
        )
}

/// The result text an `edit` call leaves in the transcript: the path it changed
/// and the diff it made, as the handler returned them.
fn edit_result(path: &str, diff: &str) -> String {
    // `serde_json` is not a test dependency here: build the handler's own shape.
    let escaped = diff
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    format!("{{\"path\":\"{path}\",\"diff\":\"{escaped}\"}}")
}

/// A two-turn persisted session: prose, a finished tool call with its result, a
/// compaction summary, and prose after it.
fn persisted_session() -> Vec<UiMessageSnapshot> {
    let diff = "--- a\n+++ b\n";
    vec![
        UiMessageSnapshot::new("user", "first question"),
        assistant("first answer", Some(usage(3, 2, 5))),
        tool(
            "edit",
            r#"{"path":"a.rs"}"#,
            &edit_result("a.rs", diff),
            Some(true),
        ),
        UiMessageSnapshot::new("user", "second question"),
        UiMessageSnapshot::new("compaction", "## Summary\n- alpha"),
        assistant("third answer", None),
    ]
}

/// Fold the projected log the way the live session folds it: one reduce per
/// record, the effect queue drained after each, exactly as the coordinator does.
fn live_reduction(messages: &[UiMessageSnapshot]) -> AppState {
    let cancel_controller = CancelController::default();
    let mut state = AppState::default();
    let (records, _total) = project_snapshots(messages.to_vec());
    for record in records {
        reduce(&mut state, RenderInput::Record(&record), None);
        let _ = state.take_pending_events(&cancel_controller);
        while state.take_next_launch_request().is_some() {}
    }
    state
}

/// The projection's record log.
fn projected_records(messages: Vec<UiMessageSnapshot>) -> Vec<SessionRecord> {
    project_snapshots(messages).0
}

/// The displayed token counters, in the order the status line reads them.
fn token_state(state: &AppState) -> (Option<u64>, Option<u64>, Option<u64>, u64) {
    let tokens = &state.status.tokens;
    (
        tokens.latest_input_tokens,
        tokens.latest_output_tokens,
        tokens.latest_total_tokens,
        tokens.session_total_tokens,
    )
}

fn entry_roles(state: &AppState) -> Vec<&'static str> {
    state
        .transcript
        .entries()
        .iter()
        .map(|entry| match entry {
            TranscriptEntry::Message(message) if message.role == MessageRole::User => "user",
            TranscriptEntry::Message(_) => "assistant",
            TranscriptEntry::Tool(_) => "tool",
            TranscriptEntry::Task(_) => "task",
            TranscriptEntry::Notice(_) => "notice",
            TranscriptEntry::Banner(_) => "banner",
        })
        .collect()
}

/// Every user message row in the transcript, in order.
fn user_rows(state: &AppState) -> Vec<&MessageEntry> {
    state
        .transcript
        .entries()
        .iter()
        .filter_map(|entry| match entry {
            TranscriptEntry::Message(message) if message.role == MessageRole::User => Some(message),
            _ => None,
        })
        .collect()
}

// endregion: --- Fixtures

// region:    --- Modules

mod projection;
mod replay;

// endregion: --- Modules
