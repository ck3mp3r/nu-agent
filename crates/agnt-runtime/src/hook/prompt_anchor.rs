//! Where the prompt a run dispatches sits in the history that run reports.
//!
//! rig mints a conversation id for every entry it appends, but the runtime
//! never sees one: a run's history surfaces as plain messages, and the ids
//! `Conversation::extend` assigns at run end are discarded there. The run start
//! is the one point where rig names the prompt and the history before it, so
//! the hook records the prompt's place from what that boundary carries.
//!
//! Recovery slices a stopped turn's delta at that place. An earlier prompt with
//! the same text cannot move it, and rewriting the prompt text cannot invalidate
//! it.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

// region:    --- Types

/// Where the message a turn dispatched sits in the history the turn reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PromptAnchor(usize);

/// The place the current run's prompt holds, shared with the turn executor.
#[derive(Clone, Default)]
pub(crate) struct DispatchedPrompt {
    cell: Arc<Mutex<Option<PromptAnchor>>>,
}

// endregion: --- Types

// region:    --- Implementations

impl PromptAnchor {
    /// The anchor of a prompt that sits `index` messages into the reported
    /// history.
    pub(crate) fn at(index: usize) -> Self {
        Self(index)
    }

    /// The prompt's index in the reported history.
    pub(crate) fn index(self) -> usize {
        self.0
    }
}

impl DispatchedPrompt {
    /// Record where the run's prompt sits.
    ///
    /// `index` is the length of the input history the run starts from: the run
    /// appends the prompt directly after that prefix, and that prefix is the
    /// head of the history the run reports.
    pub(crate) fn stamp(&self, index: usize) {
        *self.lock_cell() = Some(PromptAnchor::at(index));
    }

    /// Where the last run's prompt sits, or `None` when no run dispatched one.
    pub(crate) fn anchor(&self) -> Option<PromptAnchor> {
        *self.lock_cell()
    }

    /// The cell, recovered if a writer panicked mid-update.
    fn lock_cell(&self) -> MutexGuard<'_, Option<PromptAnchor>> {
        self.cell.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

// endregion: --- Implementations
