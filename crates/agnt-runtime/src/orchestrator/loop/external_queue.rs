//! The loop's own queue of the prompts that arrive from outside the surface.

// region:    --- Modules

use std::collections::VecDeque;

// endregion: --- Modules

// region:    --- Types

/// One prompt that arrived from outside the surface, with the task whose turn
/// answers it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExternalPrompt {
    pub(crate) task_id: String,
    pub(crate) prompt: String,
}

/// What a cancel does to the task it names.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CancelDisposition {
    /// The task's turn is in flight: the cancel stops that turn now.
    StopsInFlight,
    /// The task's turn has not begun: the cancel waits, and stops the turn the
    /// moment it begins.
    WaitsForDispatch,
}

/// The loop's external intake: the prompts it holds in arrival order, the task
/// whose turn is in flight, and the cancel that waits for a turn to begin.
///
/// The queue is orchestration truth. A prompt arrives from outside the surface,
/// waits behind the turn that runs, and is dispatched by the loop from here —
/// one task finishes before the next begins. Every entry carries the id its turn
/// is attributed by, so no map and no surface stand between the arrival and the
/// dispatch.
///
/// A cancel that arrives before its task's turn begins is dispatch ordering, not
/// a stage's business, so the queue holds it too: the dispatch it names honours
/// it, exactly once.
#[derive(Default)]
pub(crate) struct ExternalQueue {
    pending: VecDeque<ExternalPrompt>,
    in_flight: Option<String>,
    /// The task a cancel named before its turn began.
    awaiting_cancel: Option<String>,
}

// endregion: --- Types

// region:    --- Implementations

impl ExternalQueue {
    /// Admit a prompt that arrived from outside the surface.
    ///
    /// Returns `false` when this loop already holds the task — running now, or
    /// queued behind the running turn — so a repeated arrival cannot hand one
    /// task a second turn.
    pub(crate) fn admit(&mut self, task_id: String, prompt: String) -> bool {
        if self.in_flight.as_deref() == Some(task_id.as_str())
            || self.pending.iter().any(|held| held.task_id == task_id)
        {
            return false;
        }
        self.pending.push_back(ExternalPrompt { task_id, prompt });
        true
    }

    /// Cancel the task `task_id` names.
    ///
    /// A cancel naming the task whose turn is in flight stops that turn now. Any
    /// other task's turn has not begun — the task may not even have arrived — so
    /// the cancel waits, and stops that turn the moment it begins. The queue
    /// holds one waiting cancel: a cancel for another task replaces the one
    /// already waiting.
    pub(crate) fn cancel(&mut self, task_id: String) -> CancelDisposition {
        if self.in_flight.as_deref() == Some(task_id.as_str()) {
            return CancelDisposition::StopsInFlight;
        }
        self.awaiting_cancel = Some(task_id);
        CancelDisposition::WaitsForDispatch
    }

    /// Empty the queue for a user cancel: return every pending prompt in
    /// arrival order, and drop any cancel that still waits for a turn to
    /// begin.
    ///
    /// The user's cancel overrides anything in flow: no queued foreign turn
    /// runs after it, and the waiting cancel dies with the queue, so a later
    /// repeat arrival of the same task id cannot fire a phantom cancel at its
    /// new dispatch.
    pub(crate) fn drain(&mut self) -> Vec<ExternalPrompt> {
        let drained = self.pending.drain(..).collect();
        self.awaiting_cancel = None;
        drained
    }

    /// Take the next prompt to dispatch, in arrival order.
    pub(crate) fn take_next(&mut self) -> Option<ExternalPrompt> {
        self.pending.pop_front()
    }

    /// Mark `task_id` as the task whose turn is in flight, and report whether a
    /// cancel waited for it.
    ///
    /// A cancel that named this task before its turn began is consumed here: the
    /// turn it named is the turn it stops, so the caller requests the cancel for
    /// this turn and for no other.
    pub(crate) fn begin_turn(&mut self, task_id: String) -> bool {
        let cancels_at_entry = self.awaiting_cancel.as_deref() == Some(task_id.as_str());
        if cancels_at_entry {
            self.awaiting_cancel = None;
        }
        self.in_flight = Some(task_id);
        cancels_at_entry
    }

    /// The turn in flight finished.
    ///
    /// Turns are serial, so the finished turn is the one in flight whatever id
    /// its result carries: the binding retires with the turn that held it.
    pub(crate) fn finish_turn(&mut self) {
        self.in_flight = None;
    }
}

// endregion: --- Implementations

// region:    --- Tests

#[cfg(test)]
#[path = "../../../test/orchestrator/loop/external_queue.rs"]
mod external_queue_test;

// endregion: --- Tests
