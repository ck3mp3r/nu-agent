//! The render loop and its fold order.
//!
//! The loop is the surface's single consumer of the bus concerns it shows. It
//! owns the rule that makes the three record lanes read as one ordered log: a
//! chunk lane (text and narration) folds before a record from another lane that
//! can close the streamed block ([`fold_text_lane`],
//! [`fold_narration_lane`]).
//!
//! It owns the input rule too: the select is `biased;` with the terminal arm
//! first, so a keypress outranks every bus lane that is ready at the same wake,
//! and one pass folds at most [`INPUT_BURST_DRAIN_MAX`] terminal events before
//! it flushes and renders once. Type-ahead past that bound waits for the next
//! pass, which keeps the bus lanes from starving behind a burst of keys.

use std::time::Instant;

use agnt_runtime::bus::{ReceiveError, Subscription};
use agnt_runtime::orchestrator::events::OrchestratorEvent;
use agnt_runtime::protocol::events::{NarrationEvent, SessionRecord};
use nu_protocol::LabeledError;
use tokio::sync::mpsc;

use super::{INPUT_BURST_DRAIN_MAX, RENDER_TICK, RenderLoopChannels};
use crate::interaction::cancel::CancelController;
use crate::interaction::input::TerminalEvent;
use crate::runtime::RuntimeCoordinator;
use crate::runtime::render::current_time_millis;

/// A terminal event whose fold plus repaint takes longer than this reads as a
/// late keypress. The warn goes to the instance log, where a failing run leaves
/// its evidence.
const INPUT_SLOW_DISPATCH_WARN_MS: u128 = 100;

/// Forward any pending picker theme selections to the command layer for
/// persistence. The TUI never writes the preference file itself.
async fn drain_theme_persist(
    coordinator: &mut RuntimeCoordinator,
    theme_persist_tx: &Option<mpsc::Sender<String>>,
) {
    let Some(tx) = theme_persist_tx else {
        return;
    };
    while let Some(name) = coordinator.state.take_next_theme_persist_request() {
        if tx.send(name).await.is_err() {
            return;
        }
    }
}

/// Render loop driving `RuntimeCoordinator` from terminal events, bus
/// channels, and a periodic tick. Owns `&mut RuntimeCoordinator` and calls
/// `AppState` methods directly.
pub(crate) async fn run_render_loop<
    B: ratatui::backend::Backend + crate::runtime::backend::FrameSync,
>(
    coordinator: &mut RuntimeCoordinator,
    cancel_controller: &CancelController,
    event_tx: mpsc::Sender<OrchestratorEvent>,
    mut terminal_event_rx: mpsc::Receiver<TerminalEvent>,
    live_terminal: &mut Option<&mut ratatui::Terminal<B>>,
    mut channels: RenderLoopChannels<'_>,
) {
    let mut stream_rx = channels.stream.subscribe();
    let mut text_rx = channels.stream.subscribe_text();
    let mut narration_rx = channels.narration.subscribe();
    let mut control_rx = channels.control.subscribe();
    let mut lifecycle_rx = channels.lifecycle.subscribe();
    let mut facts_rx = channels.facts.subscribe();
    let mut render_timer = tokio::time::interval(RENDER_TICK);

    loop {
        tokio::select! {
            // `biased;` with the terminal arm first: input outranks every bus
            // lane. Unbiased, tokio picks any ready arm, so under a streaming
            // load a keypress waits behind records that are already readable.
            biased;
            maybe = terminal_event_rx.recv() => {
                // One pass folds a bounded burst of input and then flushes and
                // renders once: type-ahead costs one frame, not one per key.
                // The tail past the bound stays queued, so the bus lanes still
                // get an arm and never starve.
                let Some(event) = maybe else { return; };
                let started = Instant::now();
                coordinator.handle_terminal_event(event);
                let mut folded = 1;
                while folded < INPUT_BURST_DRAIN_MAX {
                    let Ok(event) = terminal_event_rx.try_recv() else {
                        break;
                    };
                    coordinator.handle_terminal_event(event);
                    folded += 1;
                }
                let stopping = flush_pending(coordinator, &event_tx, cancel_controller, &channels).await;
                if stopping {
                    return;
                }
                let _ = coordinator.render_if_needed(live_terminal);
                let elapsed_ms = started.elapsed().as_millis();
                if elapsed_ms > INPUT_SLOW_DISPATCH_WARN_MS {
                    log::warn!("tui-input-slow: terminal event fold took {elapsed_ms} ms");
                }
            }
            record = stream_rx.recv() => {
                // A lag re-syncs the subscription at the tail: the dropped
                // records are counted and gone, and nothing needs repair.
                match record {
                    Ok(record) => {
                        // The chunk lanes fold first: the chunks published
                        // before this record are already on their lanes, so they
                        // reach the streamed block before a record that closes it.
                        if fold_text_lane(
                            coordinator,
                            &mut text_rx,
                            None,
                            &event_tx,
                            cancel_controller,
                            &channels,
                        )
                        .await
                        {
                            return;
                        }
                        if fold_narration_lane(
                            coordinator,
                            &mut narration_rx,
                            None,
                            &event_tx,
                            cancel_controller,
                            &channels,
                        )
                        .await
                        {
                            return;
                        }
                        // Coalesce the burst that queued behind the awaited record:
                        // one chunk fold per wake instead of one per chunk. A lag
                        // ends the drain — the buffered records arrive on later
                        // iterations, and a chunk carries `aggregated`, so a dropped
                        // older chunk loses no content.
                        let mut batch = vec![record];
                        while let Ok(next) = stream_rx.try_recv() {
                            batch.push(next);
                        }
                        // A turn completion (or failure) clears the active prompt.
                        // Drain any prompts stacked during the turn into
                        // PromptSubmitted events now, without waiting for the next
                        // terminal input.
                        if coordinator.reduce_record_batch(batch)
                            && flush_pending(coordinator, &event_tx, cancel_controller, &channels).await
                        {
                            return;
                        }
                    }
                    Err(ReceiveError::Lagged(dropped)) => {
                        log::warn!("{}", log_lag("stream", dropped));
                    }
                    Err(ReceiveError::Closed) => {}
                }
                let _ = coordinator.render_if_needed(live_terminal);
            }
            record = text_rx.recv() => {
                // The text lane carries the per-delta records alone, so a lag
                // costs no content: every chunk carries the whole text so far,
                // and the same coalescing rule folds the burst behind it.
                match record {
                    Ok(record) => {
                        if fold_text_lane(
                            coordinator,
                            &mut text_rx,
                            Some(record),
                            &event_tx,
                            cancel_controller,
                            &channels,
                        )
                        .await
                        {
                            return;
                        }
                    }
                    Err(ReceiveError::Lagged(dropped)) => {
                        log::warn!("{}", log_lag("text", dropped));
                    }
                    Err(ReceiveError::Closed) => {}
                }
                let _ = coordinator.render_if_needed(live_terminal);
            }
            event = narration_rx.recv() => {
                // Narration is the oldest-droppable tail: the surviving events
                // keep their receipt order, and a lagged subscription resumes at
                // the tail.
                match event {
                    Ok(event) => {
                        // The text lane folds first here too: a narration chunk
                        // renders the same streamed block, and a turn error closes
                        // it, so the chunks already on their lane go first.
                        if fold_text_lane(
                            coordinator,
                            &mut text_rx,
                            None,
                            &event_tx,
                            cancel_controller,
                            &channels,
                        )
                        .await
                        {
                            return;
                        }
                        if fold_narration_lane(
                            coordinator,
                            &mut narration_rx,
                            Some(event),
                            &event_tx,
                            cancel_controller,
                            &channels,
                        )
                        .await
                        {
                            return;
                        }
                    }
                    Err(ReceiveError::Lagged(dropped)) => {
                        log::warn!("{}", log_lag("narration", dropped));
                    }
                    Err(ReceiveError::Closed) => {}
                }
                let _ = coordinator.render_if_needed(live_terminal);
            }
            command = control_rx.recv() => {
                // Control carries commands addressed to whichever owner names
                // them: the surface folds the ones it owns, and the loop's own
                // intake never reaches the fold.
                if let Ok(command) = command
                    && coordinator.reduce_control(&command)
                    && flush_pending(coordinator, &event_tx, cancel_controller, &channels).await
                {
                    return;
                }
                let _ = coordinator.render_if_needed(live_terminal);
            }
            event = lifecycle_rx.recv() => {
                // The turn spine is the surface's clock over a replay batch: the
                // first live turn that starts after a switch ends the batch, and
                // the gate becomes the only card source again. A lag skips whole
                // spine steps; a skipped step is never guessed at.
                if let Ok(event) = event
                    && coordinator.reduce_lifecycle(&event)
                    && flush_pending(coordinator, &event_tx, cancel_controller, &channels).await
                {
                    return;
                }
                let _ = coordinator.render_if_needed(live_terminal);
            }
            fact = facts_rx.recv() => {
                // A lag skips snapshots, never a state: the subscription
                // resumes at the tail carrying each field's latest value, so a
                // lagged receive needs no repair of its own.
                if let Ok(fact) = fact
                    && coordinator.reduce_fact_record(fact)
                {
                    coordinator.mark_render_needed();
                    let _ = coordinator.render_if_needed(live_terminal);
                }
            }
            _ = channels.branch_rx.recv() => {
                coordinator.refresh_repo_branch();
                let _ = coordinator.render_if_needed(live_terminal);
            }
            _ = render_timer.tick() => {
                // The picker catalogs arrive on the surface's own reply hands,
                // outside the bus, so the tick is what drains them.
                coordinator.poll_picker_replies();
                if coordinator.has_pending_status_message()
                    && coordinator.expire_status_message_if_due(Instant::now())
                {
                    coordinator.mark_render_needed();
                }
                if coordinator.spinner_cadence_advanced(current_time_millis()) {
                    coordinator.mark_render_needed();
                }
                let _ = coordinator.render_if_needed(live_terminal);
            }
        }
    }
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
async fn fold_text_lane(
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
        && flush_pending(coordinator, event_tx, cancel_controller, channels).await
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
async fn fold_narration_lane(
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
        && flush_pending(coordinator, event_tx, cancel_controller, channels).await
}

/// Drain the state's outbound effects: the prompts and commands the loop owns,
/// the theme preference the command layer persists, and the two hands that end
/// the loop (quit and fatal error).
///
/// Returns whether the render loop must stop.
async fn flush_pending(
    coordinator: &mut RuntimeCoordinator,
    event_tx: &mpsc::Sender<OrchestratorEvent>,
    cancel_controller: &CancelController,
    channels: &RenderLoopChannels<'_>,
) -> bool {
    let pending = coordinator.state.take_pending_events(cancel_controller);
    for ev in pending {
        if event_tx.send(ev).await.is_err() {
            return true;
        }
    }
    drain_theme_persist(coordinator, &channels.theme_persist_tx).await;
    if coordinator.state.quit_requested {
        let _ = event_tx.send(OrchestratorEvent::Quit).await;
        return true;
    }
    if let Some(msg) = coordinator.fatal_error() {
        let _ = event_tx
            .send(OrchestratorEvent::FatalError(LabeledError::new(
                msg.to_string(),
            )))
            .await;
        return true;
    }
    false
}

// region:    --- Support

/// The warn line a lagged ring lane emits. The lane names where the loss
/// happened; the count is what the ring reported, never an estimate.
pub(super) fn log_lag(lane: &'static str, dropped: u64) -> String {
    format!("tui-lag: {lane} lane lagged: {dropped} record(s) dropped; re-synced at tail")
}

// endregion: --- Support
