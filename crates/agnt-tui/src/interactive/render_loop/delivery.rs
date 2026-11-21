//! The loop's eventual-delivery lane.
//!
//! The loop's state is the source of truth and the outbound channel is a delivery
//! surface: every send is a try-send, an effect leaves the queue only after it
//! lands, a full channel keeps it pending for the next wake or tick, and a closed
//! one ends the loop.

use agnt_runtime::orchestrator::events::OrchestratorEvent;
use nu_protocol::LabeledError;
use tokio::sync::mpsc;

use super::RenderLoopChannels;
use crate::interaction::cancel::CancelController;
use crate::runtime::RuntimeCoordinator;
use crate::state::OutboundDelivery;

/// Forward any pending picker theme selections to the command layer for
/// persistence. The TUI never writes the preference file itself.
///
/// Delivery is eventual here too: a full channel keeps the selection pending for
/// the next flush, and a closed one drops it with one warn — the command layer is
/// fire-and-forget for a preference.
fn drain_theme_persist(
    coordinator: &mut RuntimeCoordinator,
    theme_persist_tx: &Option<mpsc::Sender<String>>,
) {
    let Some(tx) = theme_persist_tx else {
        return;
    };
    loop {
        let Some(name) = coordinator.state.theme_persist_request() else {
            return;
        };
        match tx.try_send(name.to_string()) {
            Ok(()) => coordinator.state.delivered_theme_persist_request(),
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => return,
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                log::warn!(
                    "theme persist channel is closed: the theme preference stays unpersisted"
                );
                while coordinator.state.theme_persist_request().is_some() {
                    coordinator.state.delivered_theme_persist_request();
                }
                return;
            }
        }
    }
}

/// Drain the state's outbound effects: the prompts and commands the loop owns,
/// the theme preference the command layer persists, and the two hands that end
/// the loop (quit and fatal error).
///
/// Every send is a try-send, so no consumer can stall the loop: the queue's
/// front stays put while the channel is full and retries on the next flush —
/// each wake and each tick. A closed channel means the consumer is gone, which
/// still ends the loop.
///
/// Returns whether the render loop must stop.
pub(super) fn flush_pending(
    coordinator: &mut RuntimeCoordinator,
    event_tx: &mpsc::Sender<OrchestratorEvent>,
    cancel_controller: &CancelController,
    channels: &RenderLoopChannels<'_>,
) -> bool {
    coordinator.state.enqueue_pending_events(cancel_controller);
    retry_pending_delivery(coordinator, event_tx, channels)
}

/// Retry what is already queued, without pulling anything new from the state.
///
/// The fold lanes call [`flush_pending`], which enqueues first; the tick calls
/// this, so an idle tick retries a blocked delivery and never drains a source.
///
/// Returns whether the render loop must stop.
pub(super) fn retry_pending_delivery(
    coordinator: &mut RuntimeCoordinator,
    event_tx: &mpsc::Sender<OrchestratorEvent>,
    channels: &RenderLoopChannels<'_>,
) -> bool {
    loop {
        match coordinator.state.try_deliver_outbound(event_tx) {
            OutboundDelivery::Sent => continue,
            OutboundDelivery::Empty => break,
            OutboundDelivery::Full => break,
            OutboundDelivery::Closed => return true,
        }
    }
    drain_theme_persist(coordinator, &channels.theme_persist_tx);
    if coordinator.state.quit_requested {
        return deliver_stop_event(
            coordinator,
            event_tx,
            OrchestratorEvent::Quit,
            "tui-quit-blocked",
        );
    }
    if let Some(msg) = coordinator.fatal_error() {
        let event = OrchestratorEvent::FatalError(LabeledError::new(msg.to_string()));
        return deliver_stop_event(coordinator, event_tx, event, "tui-fatal-blocked");
    }
    false
}

/// Hand the event that ends the loop to the orchestrator, retrying while the
/// channel is full.
///
/// A full channel keeps the loop alive and responsive: this wake returns so keys,
/// records, and paints keep flowing, and the next flush retries. The give-up
/// deadline bounds that retry — past it the consumer is wedged, so one warn
/// records it and the loop stops.
fn deliver_stop_event(
    coordinator: &mut RuntimeCoordinator,
    event_tx: &mpsc::Sender<OrchestratorEvent>,
    event: OrchestratorEvent,
    warn_tag: &'static str,
) -> bool {
    match event_tx.try_send(event) {
        Ok(()) => {
            coordinator.clear_quit_delivery_deadline();
            true
        }
        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => true,
        Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
            if coordinator.quit_delivery_expired() {
                log::warn!("{warn_tag}: the orchestrator did not take the stop event; exiting");
                return true;
            }
            false
        }
    }
}
