//! The loop's buffer of the completion notifications it holds for an idle cycle.

// region:    --- Modules

use std::collections::VecDeque;

use crate::orchestrator::external::ExternalCompletion;

// endregion: --- Modules

// region:    --- Types

/// The completion notifications the loop could not run yet, in arrival order.
///
/// A completion cannot run while the worker is busy, so it waits here instead of
/// being dropped. The buffer keeps the order the notifications arrived, so the
/// turn that runs is always the one that has waited longest. It holds one entry
/// per remote task: a repeated notification for the same remote task updates
/// that entry in place, so the repeat neither queues a second turn nor loses its
/// place in the line.
#[derive(Default)]
pub(crate) struct CompletionBuffer {
    entries: VecDeque<ExternalCompletion>,
}

// endregion: --- Types

// region:    --- Implementations

impl CompletionBuffer {
    /// Hold a completion, in arrival order.
    ///
    /// A completion for a remote task the buffer already holds replaces that
    /// entry and keeps its position, so the dispatch order does not depend on
    /// which notification arrived last.
    pub(crate) fn push(&mut self, completion: ExternalCompletion) {
        match self
            .entries
            .iter_mut()
            .find(|held| held.task_id == completion.task_id)
        {
            Some(held) => *held = completion,
            None => self.entries.push_back(completion),
        }
    }

    /// Take the completion that has waited longest.
    pub(crate) fn take_next(&mut self) -> Option<ExternalCompletion> {
        self.entries.pop_front()
    }

    /// Whether no completion waits for a turn.
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The remote task ids still held, in dispatch order.
    pub(crate) fn task_ids(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|entry| entry.task_id.as_str())
    }

    /// Empty the buffer for a user cancel: return every buffered completion in
    /// arrival order.
    ///
    /// The user's cancel overrides anything in flow: no turn the buffer holds
    /// dispatches after it. A completion carries a remote task only, so the
    /// drain hands the entries to the caller and the caller names each
    /// abandoned remote task the only record the loop can leave.
    pub(crate) fn drain(&mut self) -> Vec<ExternalCompletion> {
        self.entries.drain(..).collect()
    }
}

// endregion: --- Implementations

// region:    --- Tests

#[cfg(test)]
#[path = "../../../test/orchestrator/loop/pending_completions.rs"]
mod pending_completions_test;

// endregion: --- Tests
