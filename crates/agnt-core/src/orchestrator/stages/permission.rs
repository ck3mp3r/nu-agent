use crate::orchestrator::stages::{OrchestrationContext, PermissionHandler};
use crate::protocol::event::PermissionDecisionSubmission;
use crate::protocol::events::{RequestId, SystemEvent};

/// The only reason a submitted decision is ignored in v1.
///
/// Neither emission site can tell a timeout from a cancel from an
/// already-resolved request, so the reason set is closed at this one value.
const REASON_REQUEST_NOT_PENDING: &str = "request_not_pending";

/// Forwards pending permission decisions from the UI to the interactive
/// permission resolver's pending map, unblocking the agent's `resolve()` future.
#[derive(Default)]
pub(crate) struct PermissionStage;

impl PermissionHandler for PermissionStage {
    async fn handle(
        &mut self,
        submission: PermissionDecisionSubmission,
        ctx: &mut OrchestrationContext<'_>,
    ) {
        // Wire the decision into InteractivePermissionResolver's pending map
        // so the agent's `resolve()` future is unblocked.
        let Some(pending) = ctx.pending else {
            return;
        };
        let removed = pending
            .lock()
            .expect("pending permissions lock")
            .remove(&submission.request_id);
        match removed {
            Some(tx) => {
                let _ = tx.send(submission.decision);
            }
            // The request timed out, was cancelled, or was already resolved.
            // The decision is dropped, so publish the drop to make it
            // observable. The payload carries no decision: correlate with the
            // earlier `permission_requested` event by `request_id`.
            None => {
                let _ = ctx
                    .bus
                    .ui_event()
                    .send(SystemEvent::PermissionDecisionIgnored {
                        request_id: RequestId::new(submission.request_id),
                        reason: REASON_REQUEST_NOT_PENDING.to_string(),
                    })
                    .await;
            }
        }
    }
}

// region:    --- Tests

#[cfg(test)]
#[path = "../../../test/orchestrator/stages/permission.rs"]
mod permission_test;

// endregion: --- Tests
