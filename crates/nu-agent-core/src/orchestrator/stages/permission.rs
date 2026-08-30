use crate::orchestrator::stages::{OrchestrationContext, PermissionHandler};
use crate::protocol::event::PermissionDecisionSubmission;

/// Forwards pending permission decisions from the UI to the interactive
/// permission resolver's pending map, unblocking the agent's `resolve()` future.
#[derive(Default)]
pub(crate) struct PermissionStage;

impl PermissionHandler for PermissionStage {
    fn handle(&mut self, submission: PermissionDecisionSubmission, ctx: &mut OrchestrationContext) {
        // Wire the decision into InteractivePermissionResolver's pending map
        // so the agent's `resolve()` future is unblocked.
        if let Some(pending) = ctx.pending
            && let Some(tx) = pending
                .lock()
                .expect("pending permissions lock")
                .remove(&submission.request_id)
        {
            let _ = tx.send(submission.decision);
        }
    }
}
