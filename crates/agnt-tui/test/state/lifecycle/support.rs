//! Shared fixtures for the lifecycle test modules.
//!
//! Holds the imports and the helpers every topical lifecycle test file needs.
//! Sub-files pull them in with `use super::*;`.

pub(super) use crate::interaction::reducer::{RenderInput, reduce};
pub(super) use crate::state::*;
pub(super) use agnt_render::transcript::entry::{MessageEntry, TranscriptEntry};
pub(super) use agnt_render::transcript::ir::MessageRole;
pub(super) use agnt_runtime::protocol::events::{PromptRecord, PromptStamps, SessionRecord};

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Fold one stream record through the reducer, as the render loop does.
pub(super) fn fold_record(state: &mut AppState, record: &SessionRecord) -> bool {
    reduce(state, RenderInput::Record(record), None)
}

/// Fold a peer's arrival: the prompt record the loop publishes on intake.
///
/// The record is the surface's whole view of an external task: the loop already
/// dispatched its turn, so the surface draws the row and submits nothing.
pub(super) fn enqueue_external(state: &mut AppState, task_id: &str, prompt: &str) {
    let record = SessionRecord::Prompt(PromptRecord {
        text: prompt.to_string(),
        stamps: PromptStamps::default(),
        task_id: Some(task_id.to_string()),
    });
    fold_record(state, &record);
}

/// Every user message row in the transcript, in order.
pub(super) fn user_messages(state: &AppState) -> Vec<MessageEntry> {
    state
        .transcript
        .entries()
        .iter()
        .filter_map(|entry| match entry {
            TranscriptEntry::Message(message) if message.role == MessageRole::User => {
                Some(message.clone())
            }
            _ => None,
        })
        .collect()
}

/// The markdown of every user message row in the transcript, in order.
pub(super) fn user_rows(state: &AppState) -> Vec<String> {
    state
        .transcript
        .entries()
        .iter()
        .filter_map(|entry| match entry {
            TranscriptEntry::Message(message) if message.role == MessageRole::User => {
                Some(message.markdown.clone())
            }
            _ => None,
        })
        .collect()
}

/// The active prompt is the one currently in `InProgress` status.
pub(super) fn active_prompt_id(state: &AppState) -> Option<u64> {
    state
        .prompt_items()
        .iter()
        .find(|p| p.status == PromptStatus::InProgress)
        .map(|p| p.id)
}

/// Pending prompts are those still in `Queued` status.
pub(super) fn pending_prompt_ids(state: &AppState) -> Vec<u64> {
    state
        .prompt_items()
        .iter()
        .filter(|p| p.status == PromptStatus::Queued)
        .map(|p| p.id)
        .collect()
}
