use crate::bus::PermissionEvent;
use crate::protocol::event::UiEvent;

/// What the worker event bridge should do with a `UiEvent` produced by the
/// worker thread's `ProgressUi`.
///
/// Lifecycle events (tool, LLM, warning, compaction, turn) are already
/// published on their own bus channels by the hooks and executor, and the
/// render loop subscribes to those channels directly. Re-publishing them here
/// would re-inject them into the same bus `BusForwarder` drains, causing an
/// infinite feedback loop that repeats the whole transcript. Only permission
/// events arrive exclusively via the worker's `ui_tx` and need forwarding to
/// the permission bus channel for the render loop to see them.
#[derive(Debug, Clone)]
pub enum BridgeAction {
    PublishPermission(PermissionEvent),
    Ignore,
}

/// Decide how the worker event bridge should handle a single `UiEvent`.
pub fn bridge_action(event: UiEvent) -> BridgeAction {
    match event {
        UiEvent::PermissionRequested {
            request_id,
            context,
        } => BridgeAction::PublishPermission(PermissionEvent::Requested {
            request_id,
            context: Box::new(context),
        }),
        UiEvent::PermissionDecisionSubmitted {
            request_id,
            decision,
            matched_rule_identity,
        } => BridgeAction::PublishPermission(PermissionEvent::DecisionSubmitted {
            request_id,
            decision,
            matched_rule_identity,
        }),
        UiEvent::PermissionDecisionTimedOut { request_id } => {
            BridgeAction::PublishPermission(PermissionEvent::DecisionTimedOut { request_id })
        }
        UiEvent::PermissionDecisionIgnored { request_id, reason } => {
            BridgeAction::PublishPermission(PermissionEvent::DecisionIgnored { request_id, reason })
        }
        _ => BridgeAction::Ignore,
    }
}

#[cfg(test)]
#[path = "bridge_test.rs"]
mod bridge_test;
