//! Shared fixtures for the compaction test modules.
//!
//! Holds the imports and the helpers every topical compaction test file needs.
//! Sub-files pull them in with `use super::*;`.

pub(super) use crate::interaction::reducer::{
    ReducerInput, UserAction, reduce_with_cancel_controller,
};
pub(super) use crate::state::{AppState, InputState};
pub(super) use agnt_core::bus::CompactionEvent;
pub(super) use agnt_core::transcript::ir::BlockSource;
pub(super) use agnt_core::transcript::ir::StyleHint;

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

pub(super) fn busy_state_with_clean_transcript() -> AppState {
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("run".to_string()),
        ..Default::default()
    };
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);
    let _ = state.activate_next_prompt();
    state.transcript.clear();
    // Simulate handle_llm_start which sets the lock
    state.input_locked = true;
    state
}

pub(super) fn reduce_compaction(state: &mut AppState, event: CompactionEvent) -> bool {
    let mut evicted = 0usize;
    let changed = state.compaction.reduce_compaction_event(
        &mut state.transcript,
        &mut state.status,
        &mut state.scroll,
        event,
        &mut evicted,
    );
    state.shift_bookkeeping_after_eviction(evicted);
    changed
}

pub(super) fn started(source: &str) -> CompactionEvent {
    CompactionEvent::Started {
        source: source.to_string(),
    }
}

pub(super) fn completed(
    source: &str,
    summary_preview: &str,
    summary_body: &str,
) -> CompactionEvent {
    CompactionEvent::Completed {
        source: source.to_string(),
        summary_preview: summary_preview.to_string(),
        summary_body: summary_body.to_string(),
    }
}

pub(super) fn chunk(source: &str, delta: &str, aggregated: &str) -> CompactionEvent {
    CompactionEvent::SummaryChunk {
        source: source.to_string(),
        delta: delta.to_string(),
        aggregated: aggregated.to_string(),
    }
}

/// Return raw markdown strings stored in all assistant markdown blocks.
pub(super) fn assistant_markdown_entries(state: &AppState) -> Vec<String> {
    state
        .transcript
        .blocks()
        .iter()
        .filter_map(|b| match &b.source {
            BlockSource::Markdown {
                role: agnt_core::transcript::ir::MessageRole::Assistant,
                markdown,
            } => Some(markdown.clone()),
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
