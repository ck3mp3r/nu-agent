//! Per-turn LLM usage concern.
//!
//! Accumulates the usage of every LLM call a turn makes, publisher-side: the
//! hook chain writes one call's usage as the call completes, and the turn
//! executor reads the sums when it publishes the turn-completed event. A
//! runner-side sum would undercount under channel lag, so the accumulation
//! lives where the calls are observed.

use std::sync::{Arc, Mutex};

// region:    --- Types

/// The summed usage of one turn, as the turn-completed event carries it.
#[derive(Debug, Clone, PartialEq)]
pub struct TurnUsageTotals {
    /// Input tokens summed across the turn's calls.
    pub input_tokens: u64,
    /// Output tokens summed across the turn's calls.
    pub output_tokens: u64,
    /// Per-call totals summed, each per the chain fallback (the wire's total,
    /// else input + output). `None` when no call reported a total.
    pub total_tokens: Option<u64>,
    /// Reported per-call costs summed. `None` when no call reported one.
    pub cost: Option<f64>,
    /// Whether `cost` is the whole charge rather than a lower bound: the AND of
    /// the per-call flags. A sum with one incomplete component is a lower
    /// bound. Vacuously true when no call reported a cost.
    pub cost_complete: bool,
}

/// Accumulates one turn's LLM usage.
///
/// The cells are created per turn, so a turn's sums never carry over into the
/// next one. The hook chain holds a clone and writes each call into it; the
/// turn executor holds a clone and reads the sums at the turn's end.
#[derive(Clone)]
pub struct TurnUsage {
    input_tokens: Arc<Mutex<u64>>,
    output_tokens: Arc<Mutex<u64>>,
    total_tokens: Arc<Mutex<Option<u64>>>,
    cost: Arc<Mutex<Option<f64>>>,
    cost_complete: Arc<Mutex<bool>>,
}

// endregion: --- Types

// region:    --- Default

impl Default for TurnUsage {
    /// An empty accumulator.
    ///
    /// `cost_complete` starts `true`: it is the identity of the AND, so the
    /// first call's flag decides the sum. With no calls the sum is empty, so
    /// the flag is vacuously true and `cost` is `None`.
    fn default() -> Self {
        Self {
            input_tokens: Arc::new(Mutex::new(0)),
            output_tokens: Arc::new(Mutex::new(0)),
            total_tokens: Arc::new(Mutex::new(None)),
            cost: Arc::new(Mutex::new(None)),
            cost_complete: Arc::new(Mutex::new(true)),
        }
    }
}

impl Default for TurnUsageTotals {
    /// The empty sum: what an accumulator that saw no call reports.
    fn default() -> Self {
        Self {
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: None,
            cost: None,
            cost_complete: true,
        }
    }
}

// endregion: --- Default

// region:    --- TurnUsage

impl TurnUsage {
    /// Add one completed call's usage.
    ///
    /// `total` is the call's total per the chain fallback: the wire's total
    /// when it reported one, else input + output. `cost` is `None` for a call
    /// nothing priced; `cost_complete` says whether that cost is the whole
    /// charge. Mutex poison is a fatal internal inconsistency — panicking is
    /// correct.
    pub fn add_call(
        &self,
        input_tokens: u64,
        output_tokens: u64,
        total: Option<u64>,
        cost: Option<f64>,
        cost_complete: bool,
    ) {
        *self.input_tokens.lock().unwrap() += input_tokens;
        *self.output_tokens.lock().unwrap() += output_tokens;
        if let Some(total) = total {
            let mut sum = self.total_tokens.lock().unwrap();
            *sum = Some(sum.unwrap_or(0) + total);
        }
        if let Some(cost) = cost {
            let mut sum = self.cost.lock().unwrap();
            *sum = Some(sum.unwrap_or(0.0) + cost);
        }
        let mut complete = self.cost_complete.lock().unwrap();
        *complete = *complete && cost_complete;
    }

    /// The sums of every call added so far.
    pub fn totals(&self) -> TurnUsageTotals {
        TurnUsageTotals {
            input_tokens: *self.input_tokens.lock().unwrap(),
            output_tokens: *self.output_tokens.lock().unwrap(),
            total_tokens: *self.total_tokens.lock().unwrap(),
            cost: *self.cost.lock().unwrap(),
            cost_complete: *self.cost_complete.lock().unwrap(),
        }
    }

    /// Zero the cells in place, so the next call starts a fresh sum.
    ///
    /// The cells are shared with every clone, so a reset is visible to a
    /// handle taken before it. `cost_complete` returns to the AND identity.
    /// Mutex poison is a fatal internal inconsistency — panicking is correct.
    pub fn reset(&self) {
        *self.input_tokens.lock().unwrap() = 0;
        *self.output_tokens.lock().unwrap() = 0;
        *self.total_tokens.lock().unwrap() = None;
        *self.cost.lock().unwrap() = None;
        *self.cost_complete.lock().unwrap() = true;
    }
}

// endregion: --- TurnUsage

// region:    --- Tests

#[cfg(test)]
#[path = "../../test/hook/turn_usage.rs"]
mod turn_usage_test;

// endregion: --- Tests
