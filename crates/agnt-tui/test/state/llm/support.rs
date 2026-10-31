//! Shared fixtures for the llm test modules.
//!
//! Holds the imports and the helpers every topical llm test file needs.
//! Sub-files pull them in with `use super::*;`.

pub(super) use crate::interaction::reducer::reduce;
pub(super) use crate::state::{AppState, UiPhase};
pub(super) use crate::test_support::entry_text;
pub(super) use agnt_core::protocol::event::{ToolDisplay, ToolDisplaySection};
pub(super) use agnt_core::protocol::events::{SystemEvent, Usage};
pub(super) use agnt_core::protocol::tool_args::CallLine;
pub(super) use agnt_core::transcript::entry::TranscriptEntry;
pub(super) use agnt_core::transcript::ir::MessageRole;

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

pub(super) fn busy_state_with_clean_transcript() -> AppState {
    let mut state = AppState::default();
    reduce(
        &mut state,
        SystemEvent::PromptSubmitted {
            text: "run".to_string(),
        },
        None,
    );
    let _ = state.activate_next_prompt();
    state.transcript.clear();
    // Simulate handle_llm_start which sets the lock
    state.input_locked = true;
    state
}

pub(super) fn is_assistant_entry(entry: &TranscriptEntry) -> bool {
    matches!(
        entry,
        TranscriptEntry::Message(message) if message.role == MessageRole::Assistant
    )
}

pub(super) fn reduce_llm(state: &mut AppState, event: SystemEvent) -> bool {
    reduce(state, event, None)
}

/// The non-streaming producer's shape: one chunk whose delta and aggregated
/// both carry the full text.
pub(super) fn assistant_message(text: &str) -> SystemEvent {
    SystemEvent::InferenceChunk {
        delta: text.to_string(),
        aggregated: text.to_string(),
    }
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
pub(super) fn project_spans(markdown: &str) -> Vec<(String, agnt_core::transcript::ir::StyleHint)> {
    crate::markdown::render_markdown_lines(markdown, None)
        .into_iter()
        .flat_map(|l| l.spans.into_iter())
        .map(|s| (s.text, s.hint))
        .collect()
}
