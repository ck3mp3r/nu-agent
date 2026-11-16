//! The identity of the completion call whose deltas are streaming.
//!
//! The hook chain mints one id per completion call and stamps it here before the
//! request goes out; every delta of that call names the same id, on both lanes.
//! The cell is shared with the turn executor, which publishes the tail delta a
//! call that streamed none still owes.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use agnt_kernel::InferenceId;

// region:    --- Types

/// The completion call the runtime is streaming now.
#[derive(Clone, Default)]
pub(crate) struct CurrentInference {
    cell: Arc<Mutex<Option<InferenceId>>>,
}

// endregion: --- Types

// region:    --- Implementations

impl CurrentInference {
    /// Declare the call whose deltas now stream.
    pub(crate) fn stamp(&self, inference: InferenceId) {
        *self.lock_cell() = Some(inference);
    }

    /// The call whose deltas now stream, or the last one that did.
    ///
    /// rig streams text only after the completion call that opened it, so the
    /// cell holds that call's id by the time a delta asks for it. A producer
    /// that streams without declaring a call mints one here: its chunk still
    /// renders once, in a block no other record claims.
    pub(crate) fn streaming(&self) -> InferenceId {
        let mut cell = self.lock_cell();
        *cell.get_or_insert_with(InferenceId::mint)
    }

    /// The cell, recovered if a writer panicked mid-update.
    fn lock_cell(&self) -> MutexGuard<'_, Option<InferenceId>> {
        self.cell.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

// endregion: --- Implementations
