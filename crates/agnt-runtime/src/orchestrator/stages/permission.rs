use std::collections::HashMap;
use std::sync::{MutexGuard, PoisonError};

use crate::conversation::runtime::PendingPermissions;
use crate::orchestrator::stages::{OrchestrationContext, PermissionHandler};
use crate::protocol::event::{
    PermissionDecision as ProtocolPermissionDecision, PermissionDecisionSubmission,
};
use agnt_kernel::bus::OneshotTx;
use agnt_kernel::{PermissionRecord, RequestId};

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
        let removed = lock_pending(pending).remove(&submission.request_id);
        match removed {
            Some(tx) => {
                let _ = tx.send(submission.decision);
            }
            // The request timed out, was cancelled, or was already resolved.
            // The decision is dropped, so publish the drop to make it
            // observable. The payload carries no decision: correlate with the
            // earlier `permission_requested` event by `request_id`.
            None => {
                let _ = ctx.stream.publish(agnt_kernel::SessionRecord::Permission(
                    PermissionRecord::Ignored {
                        request_id: RequestId::from_minted(submission.request_id),
                        reason: REASON_REQUEST_NOT_PENDING.to_string(),
                    },
                ));
            }
        }
    }
}

// region:    --- Support

/// The pending map, recovered if a submitter panicked mid-insert.
fn lock_pending(
    pending: &PendingPermissions,
) -> MutexGuard<'_, HashMap<String, OneshotTx<ProtocolPermissionDecision>>> {
    pending.lock().unwrap_or_else(PoisonError::into_inner)
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "../../../test/orchestrator/stages/permission.rs"]
mod permission_test;

// endregion: --- Tests
