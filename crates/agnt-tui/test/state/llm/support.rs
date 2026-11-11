//! Shared fixtures for the llm test modules.
//!
//! Holds the imports and the helpers every topical llm test file needs.
//! Sub-files pull them in with `use super::*;`.

pub(super) use crate::interaction::reducer::{RenderInput, reduce};
pub(super) use crate::state::{AppState, UiPhase};
pub(super) use crate::test_support::{
    entry_text, inference_completed, inference_started, inference_stopped, prompt_record,
    tool_completed, tool_started, turn_completed,
};
pub(super) use agnt_render::transcript::entry::TranscriptEntry;
pub(super) use agnt_render::transcript::ir::MessageRole;
pub(super) use agnt_runtime::protocol::events::{NarrationEvent, SessionRecord};

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// One fold datum of the llm tests: a stream record or a narration event.
///
/// `impl Into<LlmDatum>` on [`reduce_llm`] keeps call sites on either
/// vocabulary without a conversion at every one. The record is boxed because a
/// record is much larger than a narration event.
pub(super) enum LlmDatum {
    Record(Box<SessionRecord>),
    Narration(NarrationEvent),
}

impl From<SessionRecord> for LlmDatum {
    fn from(record: SessionRecord) -> Self {
        Self::Record(Box::new(record))
    }
}

impl From<NarrationEvent> for LlmDatum {
    fn from(event: NarrationEvent) -> Self {
        Self::Narration(event)
    }
}

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

pub(super) fn is_assistant_entry(entry: &TranscriptEntry) -> bool {
    matches!(
        entry,
        TranscriptEntry::Message(message) if message.role == MessageRole::Assistant
    )
}

pub(super) fn reduce_llm(state: &mut AppState, datum: impl Into<LlmDatum>) -> bool {
    match datum.into() {
        LlmDatum::Record(record) => reduce(state, RenderInput::Record(record.as_ref()), None),
        LlmDatum::Narration(event) => reduce(state, RenderInput::Narration(&event), None),
    }
}

/// The non-streaming producer's shape: one chunk whose delta and aggregated
/// both carry the full text.
pub(super) fn assistant_message(text: &str) -> NarrationEvent {
    agnt_runtime::protocol::events::NarrationEvent::InferenceChunk {
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
pub(super) fn project_spans(
    markdown: &str,
) -> Vec<(String, agnt_render::transcript::ir::StyleHint)> {
    crate::markdown::render_markdown_lines(markdown, None)
        .into_iter()
        .flat_map(|l| l.spans.into_iter())
        .map(|s| (s.text, s.hint))
        .collect()
}
