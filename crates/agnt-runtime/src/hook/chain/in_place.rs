//! The turn's compaction stash: the summary a side-call produced, waiting for the
//! executor's between-runs splice, and the one-attempt gate the threshold decision
//! keeps.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use rig_memory::SummaryState;

use agnt_kernel::CompactionSource;

use crate::types::Message;

// region:    --- Types

/// The compaction work this turn stashed for the executor.
#[derive(Clone, Default)]
pub struct PendingSummary {
    settled: Arc<Mutex<Option<Supersede>>>,
    attempted: Arc<Mutex<bool>>,
}

/// The side-call's summary and the messages it replaces: what the turn executor
/// splices between the two runs.
///
/// The summary comes from its own completion call, which validated the answer
/// before returning it; the executor then owns the store write.
#[derive(Clone)]
pub(crate) struct Supersede {
    replaces: Vec<Arc<Message>>,
    state: SummaryState,
    source: CompactionSource,
    reply: String,
}

// endregion: --- Types

// region:    --- Implementations

impl PendingSummary {
    /// Whether this turn already asked for a summary. One attempt per turn: the
    /// pass that stashed one re-runs, so the summary the first pass asked for is
    /// the turn's only one.
    pub(crate) fn attempted(&self) -> bool {
        *self.lock_attempted()
    }

    /// Take the summary waiting for the executor, `None` when none settled or it
    /// was taken already.
    pub(crate) fn take_settled(&self) -> Option<Supersede> {
        self.lock_settled().take()
    }

    /// Stash the summary a side-call produced, ready for the between-runs splice.
    pub(crate) fn stash_side_call(
        &self,
        replaces: Vec<Arc<Message>>,
        state: SummaryState,
        source: CompactionSource,
        reply: String,
    ) {
        log::debug!(
            "side-call summary ({:?}): replaces {} messages, {}-char reply",
            source,
            replaces.len(),
            reply.len()
        );
        *self.lock_settled() = Some(Supersede::planned(replaces, state, source, reply));
        *self.lock_attempted() = true;
    }

    /// Forget everything: the turn ends, or the summary cannot land.
    pub fn clear(&self) {
        *self.lock_settled() = None;
        *self.lock_attempted() = false;
    }

    /// The settled-summary cell, recovered if a writer panicked mid-update.
    fn lock_settled(&self) -> MutexGuard<'_, Option<Supersede>> {
        self.settled.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The spent-attempt cell, recovered if a writer panicked mid-update.
    fn lock_attempted(&self) -> MutexGuard<'_, bool> {
        self.attempted
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

impl Supersede {
    /// The summary a side-call produced, with the messages it replaces.
    fn planned(
        replaces: Vec<Arc<Message>>,
        state: SummaryState,
        source: CompactionSource,
        reply: String,
    ) -> Self {
        Self {
            replaces,
            state,
            source,
            reply,
        }
    }

    /// Why the summary is being written.
    pub fn source(&self) -> CompactionSource {
        self.source
    }

    /// The summary the side-call produced.
    pub fn reply(&self) -> &str {
        &self.reply
    }

    /// Every message the summary replaces, as the side-call read them.
    pub fn replaces(&self) -> &[Arc<Message>] {
        &self.replaces
    }

    /// The summary state the summary goes into: the tracked values the
    /// preparation collected, with the summary still empty.
    pub fn state(&self) -> &SummaryState {
        &self.state
    }
}

// endregion: --- Implementations
