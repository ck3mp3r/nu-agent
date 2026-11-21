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

use agnt_runtime::bus::ReceiveError;
use agnt_runtime::orchestrator::events::OrchestratorEvent;
use tokio::sync::mpsc;

use super::{INPUT_BURST_DRAIN_MAX, RENDER_TICK, RenderLoopChannels, STREAM_DRAIN_MAX};
use crate::interaction::cancel::CancelController;
use crate::interaction::input::TerminalInput;
use crate::runtime::RuntimeCoordinator;
use crate::runtime::render::current_time_millis;

// region:    --- Modules

mod delivery;
mod lanes;

use delivery::{flush_pending, retry_pending_delivery};
pub(super) use lanes::{cap_stream_batch, log_lag};
use lanes::{fold_narration_lane, fold_text_lane};

// endregion: --- Modules

/// A terminal event whose fold plus repaint takes longer than this reads as a
/// late keypress. The warn goes to the instance log, where a failing run leaves
/// its evidence.
const INPUT_SLOW_DISPATCH_WARN_MS: u128 = 100;

/// A stream arm — drain, fold, and flush — whose duration passes this bound
/// holds the terminal arm off exactly as a slow key fold does: the select is
/// biased, but an arm runs to completion before the input arm is polled again.
/// Same bound, because the wait a keypress suffers is the same wait.
const STREAM_SLOW_FOLD_WARN_MS: u128 = INPUT_SLOW_DISPATCH_WARN_MS;

/// A terminal event that waited longer than this between its read and its fold
/// was starved of the timeslice, not slowed by a slow fold: the fold bound
/// catches expensive work, and this one catches the OS scheduling the process
/// out from under the key.
const INPUT_WAIT_WARN_MS: u128 = 250;

/// A render tick that wakes later than this past its scheduled deadline woke
/// into a starved process: each missed cadence is the OS scheduling the loop
/// out, and the lateness is what the spinner's stutter measures.
const TICK_LATE_WARN_MS: u128 = 300;

/// Render loop driving `RuntimeCoordinator` from terminal events, bus
/// channels, and a periodic tick. Owns `&mut RuntimeCoordinator` and calls
/// `AppState` methods directly.
pub(crate) async fn run_render_loop<
    B: ratatui::backend::Backend + crate::runtime::backend::FrameSync,
>(
    coordinator: &mut RuntimeCoordinator,
    cancel_controller: &CancelController,
    event_tx: mpsc::Sender<OrchestratorEvent>,
    mut terminal_event_rx: mpsc::Receiver<TerminalInput>,
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
    // The deadline the interval's next tick is due at, tracked here because the
    // interval keeps its own copy: without it a wake cannot tell on-time from
    // far-late, and a stall leaves no evidence.
    let mut tick_deadline = tokio::time::Instant::now();

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
                let Some(input) = maybe else { return; };
                // The wait is the first event's alone: it is the one the user
                // felt, and the burst behind it waited only as long as the
                // drain takes.
                let waited_ms = input.waited_millis(Instant::now());
                let _ = coordinator.record_input_wait(waited_ms, INPUT_WAIT_WARN_MS);
                if waited_ms > INPUT_WAIT_WARN_MS {
                    log::warn!("tui-input-wait-slow: {waited_ms} ms from key read to fold");
                }
                let started = Instant::now();
                coordinator.handle_terminal_event(input.event);
                let mut folded = 1;
                while folded < INPUT_BURST_DRAIN_MAX {
                    let Ok(input) = terminal_event_rx.try_recv() else {
                        break;
                    };
                    coordinator.handle_terminal_event(input.event);
                    folded += 1;
                }
                let stopping = flush_pending(coordinator, &event_tx, cancel_controller, &channels);
                if stopping {
                    return;
                }
                let _ = coordinator.render_if_needed(live_terminal);
                let elapsed_ms = started.elapsed().as_millis();
                let _ = coordinator.record_input_arm(elapsed_ms, INPUT_SLOW_DISPATCH_WARN_MS);
                if elapsed_ms > INPUT_SLOW_DISPATCH_WARN_MS {
                    log::warn!("tui-input-slow: terminal event fold took {elapsed_ms} ms");
                }
            }
            record = stream_rx.recv() => {
                // A lag re-syncs the subscription at the tail: the dropped
                // records are counted and gone, and nothing needs repair.
                let arm_started = Instant::now();
                coordinator.begin_stream_arm();
                match record {
                    Ok(record) => {
                        // The chunk lanes fold first: the chunks published
                        // before this record are already on their lanes, so they
                        // reach the streamed block before a record that closes it.
                        let lane_started = Instant::now();
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
                        let text_lane_ms = lane_started.elapsed().as_millis();
                        coordinator
                            .stream_arm_timing_mut()
                            .record_text_lane(text_lane_ms);
                        let lane_started = Instant::now();
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
                        let narration_lane_ms = lane_started.elapsed().as_millis();
                        coordinator
                            .stream_arm_timing_mut()
                            .record_narration_lane(narration_lane_ms);
                        // Coalesce the burst that queued behind the awaited record,
                        // bounded: one arm folds at most [`STREAM_DRAIN_MAX`] records,
                        // so a firehose cannot hold the terminal arm off for the whole
                        // burst. The tail stays on the ring, in publication order, and
                        // folds on the next wake — the fold order is unchanged, and a
                        // lag still re-syncs at the tail.
                        let drain_started = Instant::now();
                        let batch = cap_stream_batch(&mut stream_rx, record, STREAM_DRAIN_MAX);
                        let drain_ms = drain_started.elapsed().as_millis();
                        coordinator.stream_arm_timing_mut().record_drain(drain_ms);
                        // A turn completion (or failure) clears the active prompt.
                        // Drain any prompts stacked during the turn into
                        // PromptSubmitted events now, without waiting for the next
                        // terminal input.
                        let fold_started = Instant::now();
                        let folded = coordinator.reduce_record_batch(batch);
                        let fold_ms = fold_started.elapsed().as_millis();
                        coordinator
                            .stream_arm_timing_mut()
                            .record_fold_phase(fold_ms);
                        if folded {
                            let flush_started = Instant::now();
                            let stopping =
                                flush_pending(coordinator, &event_tx, cancel_controller, &channels);
                            let flush_ms = flush_started.elapsed().as_millis();
                            coordinator.stream_arm_timing_mut().record_flush(flush_ms);
                            if stopping {
                                return;
                            }
                        }
                    }
                    Err(ReceiveError::Lagged(dropped)) => {
                        log::warn!("{}", log_lag("stream", dropped));
                    }
                    Err(ReceiveError::Closed) => {}
                }
                let arm_ms = arm_started.elapsed().as_millis();
                let _ = coordinator.record_stream_arm(arm_ms, STREAM_SLOW_FOLD_WARN_MS);
                if arm_ms > STREAM_SLOW_FOLD_WARN_MS {
                    log::warn!("tui-stream-slow: stream arm took {arm_ms} ms");
                    let breakdown = coordinator.stream_arm_breakdown(arm_ms);
                    log::warn!("{breakdown}");
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
                    && flush_pending(coordinator, &event_tx, cancel_controller, &channels)
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
                    && flush_pending(coordinator, &event_tx, cancel_controller, &channels)
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
                coordinator.refresh_repo_branch().await;
                let _ = coordinator.render_if_needed(live_terminal);
            }
            _ = render_timer.tick() => {
                // The lateness is this wake minus the deadline it was due at. The
                // tracked deadline then advances by whole periods, so a stall
                // logs one spike and the next wake is back on the cadence — the
                // interval's own catch-up cannot produce a warn burst.
                let woke_at = tokio::time::Instant::now();
                let late_ms = woke_at.saturating_duration_since(tick_deadline).as_millis();
                tick_deadline += RENDER_TICK;
                while tick_deadline <= woke_at {
                    tick_deadline += RENDER_TICK;
                }
                let _ = coordinator.record_tick_late(late_ms, TICK_LATE_WARN_MS);
                if late_ms > TICK_LATE_WARN_MS {
                    log::warn!("tui-tick-late: woke {late_ms} ms past deadline");
                }
                // The picker catalogs arrive on the surface's own reply hands,
                // outside the bus, so the tick is what drains them.
                coordinator.poll_picker_replies();
                // A pasteboard copy runs on the blocking pool; the tick folds
                // its outcome, so a slow pasteboard never stalls this arm.
                if coordinator.poll_clipboard_outcome().await {
                    coordinator.mark_render_needed();
                }
                // The tick is also the idle retry for every effect a busy
                // consumer left pending: it delivers what is already queued and
                // pulls nothing new, so an idle tick never drains a source.
                if retry_pending_delivery(coordinator, &event_tx, &channels) {
                    return;
                }
                if coordinator.has_pending_status_message()
                    && coordinator.expire_status_message_if_due(Instant::now())
                {
                    coordinator.mark_render_needed();
                }
                if coordinator.spinner_cadence_advanced(current_time_millis()) {
                    // The step is the tick's only change, so the frame repaints
                    // the clock-reading cells instead of recomposing: a state
                    // change raised earlier in this arm keeps its full frame.
                    coordinator.mark_clock_tick();
                }
                let _ = coordinator.render_if_needed(live_terminal);
            }
        }
    }
}
