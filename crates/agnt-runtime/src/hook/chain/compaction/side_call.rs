//! The self-contained side-call: a summary planned and asked for on its own,
//! with no patched reply to carry it.
//!
//! It runs when the in-place request is impossible, and when the in-place reply
//! cannot be used as a summary. The call is its own: the conversation goes as
//! text in one request, so nothing about this turn's request changes. Its answer
//! rides the turn's stash to the executor, which splices it between the runs.

use rig::DynModel;
use rig::catalog::ModelSpec;
use rig::conversation::Conversation;
use rig::operation::Completion;
use rig_memory::{CompactReason, CompactionPolicy};

use agnt_kernel::CompactionSource;

use super::super::PendingSummary;
use crate::conversation::compaction::Chunks;
use crate::conversation::compaction::compactor::NuCompactor;

/// Plan the summary alone, ask `model` for it, and stash it for the executor.
///
/// Returns whether a summary was stashed: `false` when there is nothing new to
/// summarize, or when the call failed — a failure is already published as
/// `CompactionRecord::Failed` by the call itself.
pub(super) async fn plan_alone(
    conversation: &Conversation,
    policy: &CompactionPolicy,
    spec: &ModelSpec,
    compactor: &NuCompactor,
    model: &DynModel<Completion>,
    source: CompactionSource,
    pending: &PendingSummary,
) -> bool {
    let state = rig_memory::summary_state(conversation);
    let plan = match policy.plan_summary(conversation, &state, &CompactReason::Threshold, spec) {
        Ok(Some(plan)) => plan,
        Ok(None) => {
            log::debug!("side-call: nothing new to summarize");
            return false;
        }
        Err(error) => {
            let message = format!("Compaction plan error: {error}");
            compactor.emit_failed(source, &message).await;
            return false;
        }
    };
    log::debug!(
        "side-call: planning alone from {:?} over {} messages",
        plan.keep_from,
        conversation.messages().len()
    );
    compactor.emit_started(source).await;
    match compactor
        .complete(model, plan.request.clone(), source, Chunks::Publish)
        .await
    {
        Ok(summary) => {
            pending.stash_side_call(plan.keep_from, plan.state, source, summary);
            true
        }
        // The call published its own `Failed`; the turn continues unpatched.
        Err(_) => false,
    }
}
