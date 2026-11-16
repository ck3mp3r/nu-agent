//! The drain drivers: forward bus traffic to the client, and run a turn while
//! streaming it.

use std::future::Future;

use agnt_kernel::SessionRecord;
use agnt_runtime::bus::{
    ControlCommand, ControlRx, NarrationEvent, ReceiveError, Subscription, TryReceiveError,
};

use crate::permission::{AcpPermissionBridge, PermissionRequestSink};

use super::support::{handle_record, send_input};
use super::types::{EventTranslator, SessionUpdateSink, TranslateInput, TurnSources};

/// Drain the session bus into `session/update` notifications until the bus
/// closes.
///
/// Subscribes a fresh receiver, so events published before the call are lost.
/// For a prompt turn, prefer [`run_turn_with_notifications`]: it owns the
/// receiver, so it can flush the events published just before the turn
/// completes.
pub async fn drain_bus_to_notifications<S, P>(
    sources: TurnSources<'_>,
    sink: &S,
    translator: &mut EventTranslator,
    bridge: &mut AcpPermissionBridge<P>,
) where
    S: SessionUpdateSink,
    P: PermissionRequestSink,
{
    let mut stream_rx = sources.stream.subscribe();
    let mut narration_rx = sources.narration.subscribe();
    drain_receiver_to_notifications(&mut stream_rx, &mut narration_rx, sink, translator, bridge)
        .await;
}

/// Drain an existing bus receiver into `session/update` notifications until the
/// bus closes.
///
/// Use this variant when the caller owns the receiver, so the same receiver can
/// be flushed after the turn completes. Permission requests are routed to
/// `bridge`; every request still awaiting a decision when the loop ends is
/// denied.
pub async fn drain_receiver_to_notifications<S, P>(
    stream_rx: &mut Subscription<SessionRecord>,
    narration_rx: &mut Subscription<NarrationEvent>,
    sink: &S,
    translator: &mut EventTranslator,
    bridge: &mut AcpPermissionBridge<P>,
) where
    S: SessionUpdateSink,
    P: PermissionRequestSink,
{
    let mut stream_open = true;
    let mut narration_open = true;
    while stream_open || narration_open {
        tokio::select! {
            received = stream_rx.recv(), if stream_open => match received {
                Ok(record) => {
                    if !handle_record(sink, translator, bridge, &record).await {
                        break;
                    }
                }
                // One concern closed. The other may still hold records the
                // bus published before it dropped, so drain that one first.
                Err(ReceiveError::Closed) => stream_open = false,
                Err(ReceiveError::Lagged(dropped)) => {
                    log::warn!(
                        "session {}: stream receiver lagged, dropped {dropped} records",
                        translator.session_id()
                    );
                }
            },
            received = narration_rx.recv(), if narration_open => match received {
                Ok(event) => {
                    if !send_input(sink, translator, TranslateInput::Narration(&event)) {
                        break;
                    }
                }
                Err(ReceiveError::Closed) => narration_open = false,
                Err(ReceiveError::Lagged(dropped)) => {
                    log::warn!(
                        "session {}: narration receiver lagged, dropped {dropped} events",
                        translator.session_id()
                    );
                }
            },
        }
    }
    bridge.deny_all_pending();
}

/// Forward every event already buffered on `rx` as a `session/update`
/// notification, without awaiting new events.
///
/// Call this after `execute_turn()` returns: the turn publishes its final
/// events before it completes, so a drain future dropped by `tokio::select!`
/// would otherwise leave them unread.
///
/// Permission requests are dropped here. The turn cannot complete while a
/// permission decision is still awaited, so a buffered request at this point
/// has no resolver left to unblock.
pub fn flush_receiver_to_notifications<S: SessionUpdateSink>(
    stream_rx: &mut Subscription<SessionRecord>,
    narration_rx: &mut Subscription<NarrationEvent>,
    sink: &S,
    translator: &mut EventTranslator,
) {
    loop {
        let more = match stream_rx.try_recv() {
            Ok(record) => send_input(sink, translator, TranslateInput::Record(&record)),
            Err(TryReceiveError::Lagged(dropped)) => {
                log::warn!(
                    "session {}: stream receiver lagged, dropped {dropped} records",
                    translator.session_id()
                );
                true
            }
            Err(TryReceiveError::Empty | TryReceiveError::Closed) => false,
        };
        if !more {
            break;
        }
    }
    loop {
        match narration_rx.try_recv() {
            Ok(event) => {
                send_input(sink, translator, TranslateInput::Narration(&event));
            }
            Err(TryReceiveError::Lagged(dropped)) => {
                log::warn!(
                    "session {}: narration receiver lagged, dropped {dropped} events",
                    translator.session_id()
                );
            }
            Err(TryReceiveError::Empty | TryReceiveError::Closed) => break,
        }
    }
}

/// Drive one prompt turn while streaming its bus events to the client.
///
/// `turn` is the `execute_turn()` future. The loop polls the turn and the bus
/// receivers together, so events reach the client as they are published. When
/// the turn completes, the receivers are flushed and the turn output is
/// returned.
///
/// `control_rx` carries the session's control intake. A compaction request is a
/// `ControlCommand`, not a record: it is the chain's auto-compaction decision,
/// addressed to the loop owner rather than to a client.
///
/// The returned flag reports whether a `ControlCommand::CompactionRequested`
/// arrived. The request is not a client-facing event: the caller dispatches it.
/// The flag is a `bool` because one
/// compaction runs per turn — a turn can trip the threshold on several
/// completions, and the extras are dropped: after the compaction the context
/// shrank, so the threshold re-evaluates on the next turn's completions.
///
/// The receivers must be subscribed before the turn starts, otherwise events
/// published before the subscription are lost. Permission requests are routed
/// to `bridge`; every request still awaiting a decision when the turn ends is
/// denied.
///
/// The drain is turn-scoped: an event published between turns reaches no
/// client, exactly like every ui event published between turns. That limit is
/// pre-existing and applies to compaction events unchanged.
pub async fn run_turn_with_notifications<S, P, F>(
    stream_rx: &mut Subscription<SessionRecord>,
    narration_rx: &mut Subscription<NarrationEvent>,
    control_rx: &mut ControlRx,
    sink: &S,
    translator: &mut EventTranslator,
    bridge: &mut AcpPermissionBridge<P>,
    turn: F,
) -> (F::Output, bool)
where
    S: SessionUpdateSink,
    P: PermissionRequestSink,
    F: Future,
{
    tokio::pin!(turn);
    let mut draining = true;
    let mut compaction_requested = false;
    let output = loop {
        tokio::select! {
            biased;
            output = &mut turn => break output,
            received = stream_rx.recv(), if draining => match received {
                Ok(record) => {
                    if !handle_record(sink, translator, bridge, &record).await {
                        draining = false;
                    }
                }
                Err(ReceiveError::Closed) => draining = false,
                Err(ReceiveError::Lagged(dropped)) => {
                    log::warn!(
                        "session {}: stream receiver lagged, dropped {dropped} records",
                        translator.session_id()
                    );
                }
            },
            received = narration_rx.recv(), if draining => match received {
                Ok(event) => {
                    if !send_input(sink, translator, TranslateInput::Narration(&event)) {
                        draining = false;
                    }
                }
                Err(ReceiveError::Closed) => draining = false,
                Err(ReceiveError::Lagged(dropped)) => {
                    log::warn!(
                        "session {}: narration receiver lagged, dropped {dropped} events",
                        translator.session_id()
                    );
                }
            },
            command = control_rx.recv(), if draining => match command {
                // The chain's auto-compaction decision: not a client-facing
                // event, so it is recorded rather than converted.
                Ok(ControlCommand::CompactionRequested { .. }) => compaction_requested = true,
                Ok(_) => {}
                Err(_receive_error) => draining = false,
            },
        }
    };
    if draining {
        flush_receiver_to_notifications(stream_rx, narration_rx, sink, translator);
        // A command the turn published before it completed is still queued:
        // the biased select breaks on the finished turn without reading it.
        while let Ok(command) = control_rx.try_recv() {
            if matches!(command, ControlCommand::CompactionRequested { .. }) {
                compaction_requested = true;
            }
        }
    }
    bridge.deny_all_pending();
    (output, compaction_requested)
}
