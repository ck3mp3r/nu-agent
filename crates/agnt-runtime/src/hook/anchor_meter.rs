//! The compaction trigger's meter: the usage a provider reported for the last
//! reply and the entry it counted through.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use rig::completion::Usage;
use rig::conversation::MessageId;
use rig_memory::Anchor;

// region:    --- Types

/// What the compaction decision meters the next request with.
///
/// The anchor is rig's: the usage the provider reported, and the entry it counted
/// through. The cells that make the pair are kept together so the decision reads
/// one consistent pair and the completion that settles writes the usage the
/// decision's entry describes.
#[derive(Clone, Default)]
pub struct AnchorMeter {
    cell: Arc<Mutex<MeterCell>>,
}

/// The values the decision, the settled completion and the model-switch gate
/// hand each other.
#[derive(Clone, Copy, Default)]
struct MeterCell {
    /// The usage a provider reported for the reply through `through`.
    anchor: Option<Anchor>,
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
    /// The anchor the next decision meters with, `None` until a completion
    /// settles against a recorded request.
    pub fn anchor(&self) -> Option<Anchor> {
        self.lock_cell().anchor
    }

    /// Record the entry the run's request counts through, which a completion's
    /// usage lands on.
    pub fn record_request(&self, through: MessageId) {
        self.lock_cell().through = Some(through);
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
