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
}

// endregion: --- Implementations
