//! The quit arm: one pass that applies what a quit still can, releases what will
//! never answer, and says whether the loop may exit.

use crate::bus::RequestCancel;
use crate::orchestrator::external::ExternalTaskStore;
use crate::orchestrator::handlers::{
    dispatch_pending_completions, fold_ui_responses, settle_compaction, settle_ui_responses,
};
use crate::orchestrator::r#loop::SourceChannels;
use crate::orchestrator::stages::{OrchestrationContext, SlashHandler, UiRequestHandler};
use crate::orchestrator::{CompletionBuffer, PendingRewrites};
use agnt_kernel::bus::LifecycleEvent;
use agnt_kernel::{CompactionRecord, CompactionSource, NoticeKind, NoticeRecord, SessionRecord};

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
    /// The transcript rewrite the spine says is running, and the source it
    /// names. The loop derives it from the rows alone.
    pub(crate) sweep: &'a mut Option<CompactionSource>,
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
        // A queued rewrite quit will never run, so it leaves the loop's hands
        // first: taken here, nothing dispatches it afterwards — not even the
        // turn's own end. Its terminal rows go out on every path below, a
        // rewrite the user asked for never ends in silence.
        let queued_compaction = self.pending.compaction.take();
        let queued_uncompaction = self.pending.uncompaction.take();
        if queued_compaction.is_some() || queued_uncompaction.is_some() {
            *self.quit_pending = true;
        }
        if *ctx.worker_active {
            // A turn owns the worker, so no sweep is active and the released
            // rows cannot be read as one.
            release_queued(ctx, queued_compaction, queued_uncompaction);
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
        if let Some(source) = *self.sweep {
            // The sweep is running and owns the worker. Quit asks it to stop;
            // the engine's cancel arm answers with its own terminal row, which
            // clears the slot. A sweep that cannot answer is released after one
            // quit window, so the exit never waits on it.
            *self.quit_pending = true;
            let _ = ctx.cancel.request_cancel().await;
            settle_compaction(self.sources.lifecycle_rx, self.sweep).await;
            if self.sweep.is_some() {
                release_compaction(ctx, source);
                *self.sweep = None;
            }
        }
        // The sweep is settled first, so the rows the queued rewrites end with
        // cannot be read as the sweep's own terminal row.
        release_queued(ctx, queued_compaction, queued_uncompaction);
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

// region:    --- Support

/// Publish the terminal rows of the rewrites quit took out of the queue.
///
/// The two are one call because every quit path releases what it took: the
/// rows go out whether the worker was busy, whether a sweep was settled, or
/// whether the queue was all quit owed.
fn release_queued(
    ctx: &mut OrchestrationContext<'_>,
    queued_compaction: Option<CompactionSource>,
    queued_uncompaction: Option<CompactionSource>,
) {
    if let Some(source) = queued_compaction {
        release_compaction(ctx, source);
    }
    if let Some(source) = queued_uncompaction {
        release_uncompaction(ctx, source);
    }
}

/// Publish the terminal rows of a compaction quit releases instead of running.
///
/// Both lanes carry it: the stream row is what a folding consumer reads and the
/// surface draws, and the spine row is what an ordered consumer folds as the
/// sweep's end.
fn release_compaction(ctx: &mut OrchestrationContext<'_>, source: CompactionSource) {
    let message = "Compaction abandoned at quit".to_string();
    let _ = ctx
        .stream
        .publish(SessionRecord::Compaction(CompactionRecord::Failed {
            source,
            message: message.clone(),
        }));
    let _ = ctx
        .lifecycle
        .publish(LifecycleEvent::CompactionFailed { source, message });
}

/// Publish the terminal rows of an uncompaction quit releases instead of
/// running: the warning a refusal draws, and the spine row that ends the undo.
fn release_uncompaction(ctx: &mut OrchestrationContext<'_>, source: CompactionSource) {
    let message = "Uncompaction abandoned at quit".to_string();
    let _ = ctx.stream.publish(SessionRecord::Notice(NoticeRecord {
        kind: NoticeKind::Warning,
        text: format!("Nothing undone: {message}"),
    }));
    let _ = ctx
        .lifecycle
        .publish(LifecycleEvent::UncompactFailed { source, message });
}

// endregion: --- Support
