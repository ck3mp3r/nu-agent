//! Per-turn LLM usage concern.
//!
//! Accumulates the usage of every LLM call a turn makes, publisher-side: the
//! hook chain writes one call's usage as the call completes, and the turn
//! executor reads the sums when it publishes the turn-completed event. A
//! runner-side sum would undercount under channel lag, so the accumulation
//! lives where the calls are observed.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

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
    /// charge. A poisoned cell is recovered: the cells are counters, still
    /// valid after a mid-update panic.
    pub fn add_call(
        &self,
        input_tokens: u64,
        output_tokens: u64,
        total: Option<u64>,
        cost: Option<f64>,
        cost_complete: bool,
    ) {
        *self.lock_input_tokens() += input_tokens;
        *self.lock_output_tokens() += output_tokens;
        if let Some(total) = total {
            let mut sum = self.lock_total_tokens();
            *sum = Some(sum.unwrap_or(0) + total);
        }
        if let Some(cost) = cost {
            let mut sum = self.lock_cost();
            *sum = Some(sum.unwrap_or(0.0) + cost);
        }
        let mut complete = self.lock_cost_complete();
        *complete = *complete && cost_complete;
    }

    /// The sums of every call added so far.
    pub fn totals(&self) -> TurnUsageTotals {
        TurnUsageTotals {
            input_tokens: *self.lock_input_tokens(),
            output_tokens: *self.lock_output_tokens(),
            total_tokens: *self.lock_total_tokens(),
            cost: *self.lock_cost(),
            cost_complete: *self.lock_cost_complete(),
        }
    }

    /// Zero the cells in place, so the next call starts a fresh sum.
    ///
    /// The cells are shared with every clone, so a reset is visible to a
    /// handle taken before it. `cost_complete` returns to the AND identity.
    pub fn reset(&self) {
        *self.lock_input_tokens() = 0;
        *self.lock_output_tokens() = 0;
        *self.lock_total_tokens() = None;
        *self.lock_cost() = None;
        *self.lock_cost_complete() = true;
    }

    /// The input-token cell, recovered if a writer panicked mid-update.
    fn lock_input_tokens(&self) -> MutexGuard<'_, u64> {
        self.input_tokens
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// The output-token cell, recovered if a writer panicked mid-update.
    fn lock_output_tokens(&self) -> MutexGuard<'_, u64> {
        self.output_tokens
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// The total-token cell, recovered if a writer panicked mid-update.
    fn lock_total_tokens(&self) -> MutexGuard<'_, Option<u64>> {
        self.total_tokens
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// The cost cell, recovered if a writer panicked mid-update.
    fn lock_cost(&self) -> MutexGuard<'_, Option<f64>> {
        self.cost.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The completeness cell, recovered if a writer panicked mid-update.
    fn lock_cost_complete(&self) -> MutexGuard<'_, bool> {
        self.cost_complete
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

// endregion: --- TurnUsage

// region:    --- Tests

#[cfg(test)]
#[path = "../../test/hook/turn_usage.rs"]
mod turn_usage_test;

// endregion: --- Tests
