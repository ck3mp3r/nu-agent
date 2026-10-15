//! Shared fixtures for the llm test modules.
//!
//! Holds the imports and the helpers every topical llm test file needs.
//! Sub-files pull them in with `use super::*;`.

pub(super) use crate::interaction::reducer::{
    ReducerInput, UserAction, reduce_with_cancel_controller,
};
pub(super) use crate::state::{AppState, InputState, UiPhase};
pub(super) use agnt_core::bus::LlmEvent;
pub(super) use agnt_core::protocol::event::{ToolDisplay, ToolDisplaySection};
pub(super) use agnt_core::protocol::tool_args::CallLine;
pub(super) use agnt_core::transcript::ir::BlockSource;

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

pub(super) fn is_assistant_block(block: &agnt_core::transcript::ir::Block) -> bool {
    matches!(
        block.source,
        BlockSource::Markdown {
            role: agnt_core::transcript::ir::MessageRole::Assistant,
            ..
        }
    )
}

pub(super) fn reduce_llm(state: &mut AppState, event: LlmEvent) -> bool {
    let (changed, evicted) = state.llm.reduce_llm_event(
        &mut state.transcript,
        &mut state.status,
        &mut state.scroll,
        &mut state.phase,
        &mut state.input_locked,
        event,
    );
    state.shift_bookkeeping_after_eviction(evicted);
    changed
}

pub(super) fn assistant_message(text: &str) -> LlmEvent {
    LlmEvent::AssistantMessage {
        text: text.to_string(),
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
pub(super) fn project_spans(markdown: &str) -> Vec<(String, agnt_core::transcript::ir::StyleHint)> {
    crate::markdown::render_markdown_lines(markdown, None)
        .into_iter()
        .flat_map(|l| l.spans.into_iter())
        .map(|s| (s.text, s.hint))
        .collect()
}
