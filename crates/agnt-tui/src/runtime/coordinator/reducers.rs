//! The coordinator's reducer passthroughs: every bus datum and surface input
//! converges here, and the stream batches fold under the chunk-coalescing rule.

use std::time::Instant;

use agnt_runtime::protocol::events::{
    CompactionRecord, InferenceRecord, NarrationEvent, SessionRecord,
};

use super::RuntimeCoordinator;
use crate::interaction::input::UiInput;
use crate::interaction::reducer::{
    RenderInput, narration_kind_label, narration_payload_bytes, record_kind_label,
    record_payload_bytes,
};

impl RuntimeCoordinator {
    /// Consume one kernel-state snapshot from the facts channel.
    ///
    /// Returns whether the fold changed the rendered state.
    pub fn reduce_fact_record(&mut self, fact: agnt_runtime::bus::FactRecord) -> bool {
        self.state.status.reduce_fact_record(fact)
    }

    /// Consume one command from the control intake.
    ///
    /// The surface acts on the commands it owns and ignores the rest: a
    /// foreign command changes nothing.
    pub(crate) fn reduce_control(&mut self, command: &agnt_runtime::bus::ControlCommand) -> bool {
        let changed = self.reduce_render_input(RenderInput::Control(command));
        if changed {
            self.mark_render_needed();
        }
        changed
    }

    /// Consume one step of the turn spine.
    ///
    /// The surface acts on the steps it owns and ignores the rest: the fold's
    /// latch closes on the first live turn after a replay batch, and a foreign
    /// step changes nothing.
    pub(crate) fn reduce_lifecycle(&mut self, event: &agnt_runtime::bus::LifecycleEvent) -> bool {
        let changed = self.reduce_render_input(RenderInput::Lifecycle(event));
        if changed {
            self.mark_render_needed();
        }
        changed
    }

    /// Consume one surface input: a key, a wheel notch, or a resize.
    pub fn reduce_input(&mut self, input: UiInput) -> bool {
        let folded = crate::interaction::reducer::reduce(
            &mut self.state,
            input,
            Some(&self.cancel_controller),
        );
        // A fold that queued a shared UI action cannot apply it inline: the key
        // lens is still borrowing the state it dispatched from.
        folded | self.state.drain_launch_requests()
    }

    /// Consume one datum from a bus concern. This is the only reducer entry
    /// point for the bus: every record, narration event, lifecycle step, and
    /// control command converges here.
    pub(crate) fn reduce_render_input(&mut self, input: RenderInput<'_>) -> bool {
        let folded = crate::interaction::reducer::reduce(
            &mut self.state,
            input,
            Some(&self.cancel_controller),
        );
        folded | self.state.drain_launch_requests()
    }

    /// Drain the catalogs the pickers' reply hands delivered since the last
    /// poll. Returns whether a catalog changed the rendered state.
    pub(crate) fn poll_picker_replies(&mut self) -> bool {
        let changed = self.state.poll_picker_replies();
        if changed {
            self.mark_render_needed();
        }
        changed
    }

    /// Fold a batch of stream records under the chunk-coalescing rule: at most
    /// the latest inference chunk and the latest compaction summary chunk are
    /// held; a pending chunk flushes before any other record so the streamed
    /// text keeps its order; a newer chunk of the same kind replaces the older
    /// one. Returns whether any fold changed state.
    pub(crate) fn reduce_record_batch(
        &mut self,
        records: impl IntoIterator<Item = SessionRecord>,
    ) -> bool {
        self.fold_batch(
            records,
            |record| {
                matches!(
                    record,
                    SessionRecord::Inference(InferenceRecord::Chunk { .. })
                )
            },
            |record| {
                matches!(
                    record,
                    SessionRecord::Compaction(CompactionRecord::SummaryChunk { .. })
                )
            },
            record_kind_label,
            record_payload_bytes,
            |coordinator, record| coordinator.reduce_render_input(RenderInput::Record(record)),
        )
    }

    /// Fold a batch of narration events under the same chunk-coalescing rule.
    pub(crate) fn reduce_narration_batch(
        &mut self,
        events: impl IntoIterator<Item = NarrationEvent>,
    ) -> bool {
        self.fold_batch(
            events,
            |event| matches!(event, NarrationEvent::InferenceChunk { .. }),
            |event| matches!(event, NarrationEvent::CompactionSummaryChunk { .. }),
            narration_kind_label,
            narration_payload_bytes,
            |coordinator, event| coordinator.reduce_render_input(RenderInput::Narration(event)),
        )
    }

    /// The coalescing skeleton both batches share: hold the newest chunk of each
    /// kind, flush the held ones before any other item, and flush the tail.
    fn fold_batch<T>(
        &mut self,
        items: impl IntoIterator<Item = T>,
        is_inference_chunk: impl Fn(&T) -> bool,
        is_compaction_chunk: impl Fn(&T) -> bool,
        kind_label: impl Fn(&T) -> &'static str,
        payload_bytes: impl Fn(&T) -> u128,
        mut fold: impl FnMut(&mut Self, &T) -> bool,
    ) -> bool {
        let mut pending_inference: Option<T> = None;
        let mut pending_compaction: Option<T> = None;
        let mut changed = false;

        for item in items {
            if is_inference_chunk(&item) {
                pending_inference = Some(item);
                continue;
            }
            if is_compaction_chunk(&item) {
                pending_compaction = Some(item);
                continue;
            }
            if let Some(pending) = pending_inference.take() {
                changed |= self.fold_one(&pending, &mut fold, &kind_label, &payload_bytes);
            }
            if let Some(pending) = pending_compaction.take() {
                changed |= self.fold_one(&pending, &mut fold, &kind_label, &payload_bytes);
            }
            changed |= self.fold_one(&item, &mut fold, &kind_label, &payload_bytes);
        }

        // Flush the tail of the batch.
        if let Some(pending) = pending_inference.take() {
            changed |= self.fold_one(&pending, &mut fold, &kind_label, &payload_bytes);
        }
        if let Some(pending) = pending_compaction.take() {
            changed |= self.fold_one(&pending, &mut fold, &kind_label, &payload_bytes);
        }

        if changed {
            self.mark_render_needed();
        }
        changed
    }

    /// Fold one item under the arm's per-kind instrumentation: the label names
    /// the record kind, and the elapsed time joins that kind's running total.
    ///
    /// The measure is the fold call alone; nothing here reads the payload, so the
    /// instrument adds no O(payload) scan.
    fn fold_one<T>(
        &mut self,
        item: &T,
        fold: &mut impl FnMut(&mut Self, &T) -> bool,
        kind_label: &impl Fn(&T) -> &'static str,
        payload_bytes: &impl Fn(&T) -> u128,
    ) -> bool {
        let started = Instant::now();
        let changed = fold(self, item);
        let elapsed_ms = started.elapsed().as_millis();
        self.stream_arm_timing
            .record_fold(kind_label(item), elapsed_ms, payload_bytes(item));
        changed
    }
}
