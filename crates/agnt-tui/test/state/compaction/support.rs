//! Shared fixtures for the compaction test modules.
//!
//! Holds the imports and the helpers every topical compaction test file needs.
//! Sub-files pull them in with `use super::*;`.

pub(super) use crate::interaction::reducer::reduce;
pub(super) use crate::state::AppState;
pub(super) use crate::test_support::entry_text;
pub(super) use agnt_core::protocol::events::SystemEvent;
pub(super) use agnt_core::transcript::entry::TranscriptEntry;
pub(super) use agnt_core::transcript::ir::MessageRole;
pub(super) use agnt_core::transcript::ir::StyleHint;

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

pub(super) fn reduce_compaction(state: &mut AppState, event: SystemEvent) -> bool {
    reduce(state, event, None)
}

pub(super) fn started(source: &str) -> SystemEvent {
    SystemEvent::CompactionStarted {
        source: source.to_string(),
    }
}

pub(super) fn completed(source: &str, summary_preview: &str, summary_body: &str) -> SystemEvent {
    SystemEvent::CompactionCompleted {
        source: source.to_string(),
        summary_preview: summary_preview.to_string(),
        summary_body: summary_body.to_string(),
    }
}

pub(super) fn chunk(source: &str, delta: &str, aggregated: &str) -> SystemEvent {
    SystemEvent::CompactionSummaryChunk {
        source: source.to_string(),
        delta: delta.to_string(),
        aggregated: aggregated.to_string(),
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
pub(super) fn project_spans(markdown: &str) -> Vec<(String, StyleHint)> {
    crate::markdown::render_markdown_lines(markdown, None)
        .into_iter()
        .flat_map(|l| l.spans.into_iter())
        .map(|s| (s.text, s.hint))
        .collect()
}
