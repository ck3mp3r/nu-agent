//! The store's measure path: the pending/stale bookkeeping, the pushed-entry
//! deferral, the in-place re-measure, and the height-index measure engine.
//!
//! One `impl TranscriptStore` block, split out of `mod.rs` for the file-size law.
//! The measurement itself stays in `height_index.rs`: these methods wire it to the
//! frame that reads it.

use std::time::Instant;

use super::*;

/// How many pending measures a fold may defer before the oldest is flushed
/// in-fold. The valve, not the path: a fold normally defers one or two entries,
/// and the repro never reaches this bound.
const PENDING_MEASURE_CAP: usize = 8;

impl TranscriptStore {
    /// Mark one entry's cached rows stale: the frame that paints it re-measures
    /// it ([`Self::ensure_height_index`]). A mark never measures — a second entry
    /// marked while one is pending must not pay a projection inside the fold, so
    /// both wait for the frame's single flush.
    ///
    /// Past [`PENDING_MEASURE_CAP`] marks the oldest is flushed in-fold instead:
    /// the degraded valve, trace-logged, that keeps the set bounded.
    pub(super) fn mark_stale(&mut self, index: usize) {
        if !self.stale_entries.contains(&index) {
            self.stale_entries.push_back(index);
        }
        while self.stale_entries.len() > PENDING_MEASURE_CAP {
            let Some(oldest) = self.stale_entries.pop_front() else {
                break;
            };
            log::trace!(
                "transcript: pending measure set is over its cap; flushing entry {oldest} in-fold"
            );
            self.remeasure_entry_at(oldest);
        }
    }

    /// Measure every pending entry once, in FIFO order, then re-derive the
    /// offsets. Returns whether any entry was measured.
    ///
    /// The frame calls this: the one pay point every fold defers to.
    pub(super) fn flush_pending_entries(&mut self) -> bool {
        let mut measured = false;
        while let Some(index) = self.stale_entries.pop_front() {
            self.remeasure_entry_at(index);
            measured = true;
        }
        measured && self.height_index_width.is_some()
    }

    /// Append the pending metrics for the entry just pushed and hand that entry
    /// to the frame's deferred measure ([`Self::ensure_height_index`]).
    ///
    /// The family and the padding are entry data, so the gap walk keeps the real
    /// entry shape; only the content rows wait for the frame. The mark joins the
    /// same pending set as a streamed fold, so a push that lands while another
    /// entry is pending defers both — nothing is measured in-fold.
    /// No-op when no index is live: the next rebuild measures everything.
    pub(super) fn defer_pushed_entry(&mut self) {
        if self.height_index_width.is_none() {
            return;
        }
        let Some(entry) = self.entries.last() else {
            return;
        };
        let index = self.entries.len() - 1;
        self.height_index.metrics.push(pending_metrics(entry));
        self.height_index.derive();
        self.mark_stale(index);
    }

    /// Re-measure one entry mutated in place at the indexed width, then
    /// re-derive the offsets. No-op when the index is stale — the next
    /// `rebuild_height_index` re-measures everything.
    ///
    /// The entry's revision moves first: it is the content that changed, and
    /// that fact holds whether or not an index is live to re-measure it.
    pub(crate) fn remeasure_entry_at(&mut self, index: usize) {
        self.bump_revision(index);
        let Some(width) = self.height_index_width else {
            return;
        };
        let Some(entry) = self.entries.get(index) else {
            return;
        };
        let started = Instant::now();
        let metrics = entry_metrics(entry, width);
        self.remeasure_millis = self
            .remeasure_millis
            .saturating_add(started.elapsed().as_millis());
        self.remeasure_calls = self.remeasure_calls.saturating_add(1);
        self.last_remeasure_index = Some(index);
        let Some(slot) = self.height_index.metrics.get_mut(index) else {
            return;
        };
        *slot = metrics;
        self.height_index.derive();
        self.remeasured_total = self.remeasured_total.saturating_add(1);
        if let Some(position) = self
            .stale_entries
            .iter()
            .position(|&marked| marked == index)
        {
            self.stale_entries.remove(position);
        }
    }

    /// Rebuild the height index from the backend's `measure(entry, width)` for
    /// every entry.
    pub(crate) fn rebuild_height_index(&mut self, width: usize) {
        self.height_index = HeightIndex::rebuild(&self.entries, width);
        self.height_index_width = Some(width);
        self.stale_entries.clear();
    }

    /// Bring the height index to the frame that is about to read it.
    ///
    /// A wholesale rebuild runs when the index is stale for `width` (a resize,
    /// or an entry added before the index existed). Otherwise every entry a fold
    /// left pending or a push left unmeasured is measured here — once each, and
    /// never the whole list — so a burst of chunks or a pushed entry costs the
    /// frame its measurements and the fold none; the call also opens the window
    /// the fold's whole-text projection runs in.
    pub(crate) fn ensure_height_index(&mut self, width: usize) {
        if !self.height_index_valid_for(width) {
            self.rebuild_height_index(width);
        }
        let measured = self.flush_pending_entries();
        self.measure_epoch = self.measure_epoch.saturating_add(1);
        if measured {
            log::trace!(
                "transcript: frame re-measured its stale entry ({0} in-place re-measures so far)",
                self.remeasured_total()
            );
        }
    }

    // region:    --- Costs

    /// Milliseconds and calls the measure path has spent re-measuring a displaced
    /// stale entry since construction.
    pub(crate) fn remeasure_cost(&self) -> (u128, u64) {
        (self.remeasure_millis, self.remeasure_calls)
    }

    /// The entry the measure path touched last, so a slow measure names WHICH.
    pub(crate) fn last_remeasure_index(&self) -> Option<usize> {
        self.last_remeasure_index
    }

    /// Milliseconds and calls the diff gate has spent re-projecting the answer
    /// since construction.
    pub(crate) fn diff_gate_cost(&self) -> (u128, u64) {
        (self.diff_gate_millis, self.diff_gate_calls)
    }

    /// Record one diff-gate projection the gate re-paid, with its duration.
    pub(crate) fn record_diff_gate_cost(&mut self, millis: u128) {
        self.diff_gate_millis = self.diff_gate_millis.saturating_add(millis);
        self.diff_gate_calls = self.diff_gate_calls.saturating_add(1);
    }

    // endregion: --- Costs
}

// region:    --- Tests

#[cfg(test)]
#[path = "../../../test/state/transcript/instrumentation.rs"]
mod instrumentation_test;

// endregion: --- Tests
