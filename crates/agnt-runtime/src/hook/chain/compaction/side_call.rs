//! The self-contained side-call: a summary planned and asked for on its own.
//!
//! It runs when the turn's request is over the threshold. The call is its own: the
//! conversation goes as text in one request, so nothing about this turn's request
//! changes. Its answer rides the turn's stash to the executor, which splices it
//! between the runs.

use rig::DynModel;
use rig::catalog::ModelSpec;
use rig::conversation::Conversation;
use rig::operation::Completion;
use rig_memory::SummaryState;

use agnt_kernel::CompactionSource;

use super::super::PendingSummary;
use crate::conversation::compaction::compactor::NuCompactor;

/// Prepare the summary alone, ask `model` for it, and stash it for the executor.
///
/// Returns whether a summary was stashed: `false` when there is nothing new to
/// summarize, or when the call failed — a failure is already published as
/// `CompactionRecord::Failed` by the call itself.
pub(super) async fn plan_alone(
    conversation: &Conversation,
    spec: &ModelSpec,
    compactor: &NuCompactor,
    model: &DynModel<Completion>,
    source: CompactionSource,
    overflowed: bool,
    pending: &PendingSummary,
) -> bool {
    let Some(preparation) = rig_memory::prepare_compaction(
        conversation,
        compactor.settings().keep_recent_tokens,
        overflowed,
    ) else {
        log::debug!("side-call: nothing new to summarize");
        return false;
    };
    log::debug!(
        "side-call: summary replaces {} of {} messages",
        preparation.replaces.len(),
        conversation.messages().len()
    );
    compactor.emit_started(source).await;
    match compactor.summarize(model, &preparation, spec, source).await {
        Ok(reply) => {
            let state = SummaryState {
                summary: String::new(),
                read_files: preparation.files.read_files,
                modified_files: preparation.files.modified_files,
            };
            pending.stash_side_call(preparation.replaces, state, source, reply);
            true
        }
        // The call published its own `Failed`; the turn continues unpatched.
        Err(_) => false,
    }
}
