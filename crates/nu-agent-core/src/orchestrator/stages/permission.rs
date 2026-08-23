use crate::orchestrator::stages::{OrchestrationContext, PermissionHandler};
use crate::protocol::{
    event::PermissionDecisionSubmission,
    permission::{SubmitOutcome, submit_active_permission_decision},
};

/// Forwards pending permission decisions from the UI to the active permission resolver.
pub(crate) struct PermissionStage;

impl PermissionStage {
    pub fn new() -> Self {
        Self
    }
}

impl PermissionHandler for PermissionStage {
    fn handle(&mut self, submission: PermissionDecisionSubmission, ctx: &mut OrchestrationContext) {
        match submit_active_permission_decision(
            submission.request_id.clone(),
            submission.decision,
            submission.matched_rule_identity.clone(),
        ) {
            SubmitOutcome::Accepted => {}
            SubmitOutcome::Ignored { reason } => {
                let _ = ctx.bus.warning().send(crate::bus::WarningEvent::Message {
                    message: format!("Permission decision ignored: {}", reason),
                });
            }
        }

        // Wire the decision into InteractivePermissionResolver's pending map
        // so the agent's `resolve()` future is unblocked.
        if let Some(ref pending) = *ctx.pending
            && let Some(tx) = pending
                .lock()
                .expect("pending permissions lock")
                .remove(&submission.request_id)
        {
            let _ = tx.send(submission.decision);
        }
    }
}
