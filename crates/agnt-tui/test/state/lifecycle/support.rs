//! Shared fixtures for the lifecycle test modules.
//!
//! Holds the imports and the helpers every topical lifecycle test file needs.
//! Sub-files pull them in with `use super::*;`.

pub(super) use crate::state::*;
pub(super) use agnt_core::orchestrator::UiStateEvent;
pub(super) use agnt_core::transcript::ir::{Block, BlockSource, MessageRole};
pub(super) use agnt_core::transcript::items::Message;
pub(super) use agnt_core::transcript::renderer::Renderable;

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

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
