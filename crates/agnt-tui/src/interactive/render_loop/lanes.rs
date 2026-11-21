//! The loop's bus-lane helpers: the bounded stream-batch drain, the two chunk
//! lanes that must fold first, and the lag warn line.

use agnt_runtime::bus::Subscription;
use agnt_runtime::orchestrator::events::OrchestratorEvent;
use agnt_runtime::protocol::events::{NarrationEvent, SessionRecord};
use tokio::sync::mpsc;

use super::RenderLoopChannels;
use super::delivery::flush_pending;
use crate::interaction::cancel::CancelController;
use crate::runtime::RuntimeCoordinator;

/// Take the records one arm folds: the record the arm already received, plus
/// the ones queued behind it, up to `cap` in all.
///
/// The tail past the cap stays on the ring, in publication order, for the next
/// wake: the ring is FIFO, so a capped drain changes how many records share one
/// arm and nothing about the order they fold in. A ring that lags re-syncs at
/// its tail exactly as an unbounded drain would — the drain just ends.
pub(in crate::interactive) fn cap_stream_batch(
    stream_rx: &mut Subscription<SessionRecord>,
    first: SessionRecord,
    cap: usize,
) -> Vec<SessionRecord> {
    let mut batch = Vec::with_capacity(cap.max(1));
    batch.push(first);
    while batch.len() < cap {
        let Ok(next) = stream_rx.try_recv() else {
            break;
        };
        batch.push(next);
    }
    batch
}

/// Drain and fold the text lane ahead of the record this wake received on
/// another lane.
///
/// The two stream lanes are separate subscriptions, so one wake can hold an
/// event-lane record and a text-lane chunk at once, and the loop's own select
/// decides which arm runs first. A turn completion closes the streamed block,
/// so a chunk folded after it pushes the whole answer into a second entry. The
/// text lane therefore drains before the event lane or the narration lane folds
/// a record: inside one turn the fold order is the publication order again,
/// which the single ring gave for free.
///
/// `pending` is the record the receiving arm already took off the lane.
///
/// Returns whether the render loop must stop.
pub(super) async fn fold_text_lane(
    coordinator: &mut RuntimeCoordinator,
    text_rx: &mut Subscription<SessionRecord>,
    pending: Option<SessionRecord>,
    event_tx: &mpsc::Sender<OrchestratorEvent>,
    cancel_controller: &CancelController,
    channels: &RenderLoopChannels<'_>,
) -> bool {
    let mut batch: Vec<SessionRecord> = pending.into_iter().collect();
    while let Ok(next) = text_rx.try_recv() {
        batch.push(next);
    }
    if batch.is_empty() {
        return false;
    }
    coordinator.reduce_record_batch(batch)
        && flush_pending(coordinator, event_tx, cancel_controller, channels)
}

/// Drain and fold the narration lane ahead of the record this wake received on
/// another lane, or fold the narration event the receiving arm already took off
/// it.
///
/// The narration lane carries the same streamed text as the text lane, so it
/// obeys the same rule: a record that closes the streamed block (`Stopped`, a
/// compaction completion) must not fold ahead of a chunk that belongs inside
/// that block. The event arm drains this lane before it folds its batch for the
/// same reason it drains the text lane.
///
/// `pending` is the event the receiving arm already took off the lane.
///
/// Returns whether the render loop must stop.
pub(super) async fn fold_narration_lane(
    coordinator: &mut RuntimeCoordinator,
    narration_rx: &mut Subscription<NarrationEvent>,
    pending: Option<NarrationEvent>,
    event_tx: &mpsc::Sender<OrchestratorEvent>,
    cancel_controller: &CancelController,
    channels: &RenderLoopChannels<'_>,
) -> bool {
    let mut batch: Vec<NarrationEvent> = pending.into_iter().collect();
    while let Ok(next) = narration_rx.try_recv() {
        batch.push(next);
    }
    if batch.is_empty() {
        return false;
    }
    coordinator.reduce_narration_batch(batch)
        && flush_pending(coordinator, event_tx, cancel_controller, channels)
}

/// The warn line a lagged ring lane emits. The lane names where the loss
/// happened; the count is what the ring reported, never an estimate.
pub(in crate::interactive) fn log_lag(lane: &'static str, dropped: u64) -> String {
    format!("tui-lag: {lane} lane lagged: {dropped} record(s) dropped; re-synced at tail")
}
