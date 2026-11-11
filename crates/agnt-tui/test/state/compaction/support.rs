//! Shared fixtures for the compaction test modules.
//!
//! Holds the imports and the helpers every topical compaction test file needs.
//! Sub-files pull them in with `use super::*;`.

pub(super) use crate::interaction::reducer::{RenderInput, reduce};
pub(super) use crate::state::{AppState, replay_records};
pub(super) use crate::test_support::{
    compaction_completed, compaction_failed, compaction_started, compaction_summary_chunk,
    entry_text,
};
pub(super) use agnt_render::transcript::entry::TranscriptEntry;
pub(super) use agnt_render::transcript::ir::MessageRole;
pub(super) use agnt_render::transcript::ir::StyleHint;
pub(super) use agnt_runtime::protocol::events::{CompactionSource, SessionRecord};

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

pub(super) fn busy_state_with_clean_transcript() -> AppState {
    let mut state = AppState::default();
    // A running turn starts at the surface's own submit lane: the record fold
    // places rows and never submits, so the queue is what carries the prompt.
    state.enqueue_prompt("run".to_string());
    let _ = state.activate_next_prompt();
    state.transcript.clear();
    // Simulate handle_llm_start which sets the lock
    state.input_locked = true;
    state
}

pub(super) fn reduce_compaction(state: &mut AppState, record: SessionRecord) -> bool {
    reduce(state, RenderInput::Record(&record), None)
}

pub(super) fn started(source: CompactionSource) -> SessionRecord {
    compaction_started(source)
}

pub(super) fn completed(source: CompactionSource, summary_body: &str) -> SessionRecord {
    compaction_completed(source, summary_body)
}

pub(super) fn failed(source: CompactionSource, message: &str) -> SessionRecord {
    compaction_failed(source, message)
}

pub(super) fn chunk(source: CompactionSource, delta: &str, aggregated: &str) -> SessionRecord {
    compaction_summary_chunk(source, delta, aggregated)
}

/// Return raw markdown strings stored in all assistant markdown entries.
pub(super) fn assistant_markdown_entries(state: &AppState) -> Vec<String> {
    state
        .transcript
        .entries()
        .iter()
        .filter_map(|entry| match entry {
            TranscriptEntry::Message(message) if message.role == MessageRole::Assistant => {
                Some(message.markdown.clone())
            }
            _ => None,
        })
        .collect()
}

/// Project a markdown string and return all (text, hint) pairs from it.
pub(super) fn project_spans(markdown: &str) -> Vec<(String, StyleHint)> {
    crate::markdown::render_markdown_lines(markdown, None)
        .into_iter()
        .flat_map(|l| l.spans.into_iter())
        .map(|s| (s.text, s.hint))
        .collect()
}
