//! The compaction trigger's meter: the usage a provider reported for the last
//! reply, the request that reply answered, and the entry it counted through.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use rig::completion::Usage;
use rig::conversation::MessageId;
use rig_memory::{Anchor, RequestStart};

// region:    --- Types

/// What the compaction decision meters the next request with.
///
/// rig keeps an anchor valid only while the request it describes is still the
/// one about to be sent, checked by a fingerprint that holds in the process that
/// made it. Our turn loads a fresh conversation per run, so the fingerprint
/// degrades to `whole_tokens()` across runs; the cells that make the pair are
/// kept together so the decision reads one consistent triple and the completion
/// that settles writes the usage the decision's entry describes.
#[derive(Clone, Default)]
pub struct AnchorMeter {
    cell: Arc<Mutex<MeterCell>>,
}

/// The values the decision, the settled completion and the model-switch gate
/// hand each other.
#[derive(Clone, Copy, Default)]
struct MeterCell {
    /// The usage a provider reported for the reply to `sent`, through `through`.
    anchor: Option<Anchor>,
    /// The request the anchor describes: what it started with.
    sent: Option<RequestStart>,
    /// The last entry of the conversation that request sent.
    through: Option<MessageId>,
    /// The tokens the last reported usage counted: the wire's total, else input
    /// plus output. Recorded for every completion, anchored or not, because the
    /// model-switch gate reads it without metering a request.
    context_tokens: Option<u64>,
}

// endregion: --- Types

// region:    --- Implementations

impl AnchorMeter {
    /// The anchor and the request start the next decision meters with.
    pub fn metering(&self) -> (Option<Anchor>, Option<RequestStart>) {
        let cell = self.lock_cell();
        (cell.anchor, cell.sent)
    }

    /// Record the request the run is about to send: what it starts with and the
    /// entry it counts through, which a completion's usage lands on.
    pub fn record_request(&self, sent: RequestStart, through: MessageId) {
        let mut cell = self.lock_cell();
        cell.sent = Some(sent);
        cell.through = Some(through);
    }

    /// Record the usage a provider reported, through the entry the decision
    /// metered. A completion whose request the decision never recorded has no
    /// entry to anchor on, so nothing is anchored there.
    pub fn record_usage(&self, usage: Usage) {
        let mut cell = self.lock_cell();
        cell.context_tokens = usage.context_tokens();
        if let Some(through) = cell.through {
            cell.anchor = Some(Anchor { usage, through });
        }
    }

    /// The tokens the last reported usage counted, `None` until a provider
    /// reports one and after a compaction. The model-switch gate reads it: a
    /// session that already consumed a model's whole window cannot fit it.
    pub fn context_tokens(&self) -> Option<u64> {
        self.lock_cell().context_tokens
    }

    /// Forget the anchor: a compaction rewrote the context, so the reported
    /// usage no longer describes what the next request sends.
    pub fn reset(&self) {
        *self.lock_cell() = MeterCell::default();
    }

    /// The cell, recovered if a writer panicked mid-update.
    fn lock_cell(&self) -> MutexGuard<'_, MeterCell> {
        self.cell.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

// endregion: --- Implementations

// region:    --- Tests

#[cfg(test)]
#[path = "../../test/hook/anchor_meter.rs"]
mod anchor_meter_test;

// endregion: --- Tests
