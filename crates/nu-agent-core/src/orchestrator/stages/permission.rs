use crate::orchestrator::stages::{OrchestrationContext, StageOutcome};
use crate::protocol::{
    contracts::{DisplayStateUi, LifecycleUi, ProgressUi, TranscriptUi, UserInputUi},
    event::UiEvent,
    permission::{SubmitOutcome, submit_active_permission_decision},
};

/// Forwards pending permission decisions from the UI to the active permission resolver.
pub(crate) struct PermissionStage;

impl PermissionStage {
    pub fn new() -> Self {
        Self
    }

    pub fn poll<U>(&mut self, ctx: &mut OrchestrationContext<'_, U>) -> StageOutcome
    where
        U: ProgressUi + UserInputUi + DisplayStateUi + LifecycleUi + TranscriptUi,
    {
        let mut handled = false;

        while let Some(submission) = ctx.ui.take_next_permission_decision_submission() {
            handled = true;
            match submit_active_permission_decision(
                submission.request_id.clone(),
                submission.decision,
                submission.matched_rule_identity.clone(),
            ) {
                SubmitOutcome::Accepted => {}
                SubmitOutcome::Ignored { reason } => {
                    ctx.ui.emit(&UiEvent::PermissionDecisionIgnored {
                        request_id: submission.request_id.clone(),
                        reason: reason.to_string(),
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

        if handled {
            StageOutcome::Handled
        } else {
            StageOutcome::Idle
        }
    }
}
