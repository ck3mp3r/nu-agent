//! The quit arm: one pass that applies what a quit still can, releases what will
//! never answer, and says whether the loop may exit.

use crate::bus::RequestCancel;
use crate::orchestrator::external::ExternalTaskStore;
use crate::orchestrator::handlers::{
    dispatch_pending_completions, fold_ui_responses, settle_ui_responses,
};
use crate::orchestrator::r#loop::SourceChannels;
use crate::orchestrator::stages::{OrchestrationContext, SlashHandler, UiRequestHandler};
use crate::orchestrator::{CompletionBuffer, PendingRewrites};

/// What the loop does after one quit pass.
pub(crate) enum QuitStep {
    /// Something a quit promised is still in flight: keep looping, and exit
    /// when it arrives.
    Wait,
    /// Nothing is owed: exit.
    Exit,
}

/// The loop state one quit pass reads and moves.
///
/// A struct rather than seven parameters: every field is a distinct piece of
/// loop state the arm writes, and the borrows stay disjoint because the caller
/// reborrows each one.
pub(crate) struct QuitArm<'a, 's, U, Sl, E>
where
    U: UiRequestHandler,
    Sl: SlashHandler,
    E: ExternalTaskStore,
{
    /// The requests a quit may still apply, release, or abandon.
    pub(crate) ui_request: &'a mut U,
    /// The buffered completion's flush runs a prompt through this stage.
    pub(crate) slash: &'a mut Sl,
    /// The response lanes a settle and a fold read.
    pub(crate) sources: &'a mut SourceChannels<'s, E>,
    /// The transcript rewrites the worker still owes.
    pub(crate) pending: &'a mut PendingRewrites,
    /// Whether a rewrite is running right now.
    pub(crate) compaction_active: &'a mut bool,
    /// The completions waiting for a turn, flushed before the exit.
    pub(crate) pending_completions: &'a mut CompletionBuffer,
    /// Set when the exit waits on something.
    pub(crate) quit_pending: &'a mut bool,
}

impl<U, Sl, E> QuitArm<'_, '_, U, Sl, E>
where
    U: UiRequestHandler,
    Sl: SlashHandler,
    E: ExternalTaskStore,
{
    /// Run one quit pass, and answer for the loop's next step.
    pub(crate) async fn apply(&mut self, ctx: &mut OrchestrationContext<'_>) -> QuitStep {
        if *ctx.worker_active {
            // The turn in flight is cancelled, and quit waits for its end, so
            // nothing is abandoned under a running turn.
            *self.quit_pending = true;
            let _ = ctx.cancel.request_cancel().await;
            return QuitStep::Wait;
        }
        // The worker is idle, so a switch that queued under a stale guard
        // applies here — one attempt, before quit reads the pending set.
        self.ui_request.drain_queued(ctx).await;
        if self.ui_request.has_pending() {
            // Responses already on their way land first; what never arrives is
            // abandoned, so quit cannot defer on an entry it will never hear
            // about.
            settle_ui_responses(self.ui_request, self.sources, ctx).await;
            self.ui_request.drain_queued(ctx).await;
        }
        if self.ui_request.has_pending() {
            self.ui_request.abandon_unanswered(ctx).await;
            // Each abandonment answered its entry on that entry's own lane.
            // Folding here releases the surface — the modal, the pickers —
            // before quit breaks, so no wait outlives the loop.
            fold_ui_responses(
                self.ui_request,
                self.sources.blocking_response_rx,
                self.sources.concurrent_response_rx,
                ctx,
            )
            .await;
        }
        if self.pending.compaction.is_some() || self.pending.uncompaction.is_some() {
            *self.quit_pending = true;
            return QuitStep::Wait;
        }
        if *self.compaction_active {
            *self.quit_pending = true;
            return QuitStep::Wait;
        }
        if !self.pending_completions.is_empty() {
            // Quit does not abandon a buffered completion. Nothing else would
            // run the flush, so start its first turn here and exit when the
            // buffer is empty.
            *self.quit_pending = true;
            dispatch_pending_completions(
                self.pending_completions,
                self.slash,
                self.ui_request,
                ctx,
            )
            .await;
            return QuitStep::Wait;
        }
        QuitStep::Exit
    }
}
