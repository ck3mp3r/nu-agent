//! Handler routines for the orchestrator loop: compaction dispatch, worker
//! result handling, external (A2A) cancellation, and optional-channel awaits.

use tokio::sync::mpsc;

use crate::bus::CancelEvent;
use crate::orchestrator::WorkerCommand;
use crate::orchestrator::stages::{
    OrchestrationContext, SessionHandler, SlashHandler, UiRequestHandler,
};
use crate::orchestrator::turn_outcome::TurnOutcome;
use crate::protocol::events::SystemEvent;

/// Reason recorded on a task rejected or failed because the worker was busy.
pub(crate) const WORKER_BUSY_REASON: &str = "Worker busy: another turn is already running";

/// Reason recorded on a task rejected because a newer prompt replaced the
/// deferred prompt that queued it.
pub(crate) const DEFERRED_PROMPT_SUPERSEDED_REASON: &str =
    "Superseded: a newer prompt replaced the deferred prompt";

/// Dispatch a compaction command to the worker, or queue it if a compaction is
/// already in flight or the worker is busy running a turn.
pub(crate) async fn dispatch_compaction(
    source: String,
    ctx: &mut OrchestrationContext<'_>,
    pending_compaction: &mut Option<String>,
    compaction_active: &mut bool,
) {
    if *compaction_active || *ctx.worker_active {
        // A compaction is already in flight or the worker is busy running a
        // turn — queue one pending compaction. A newer request replaces the
        // queued one.
        *pending_compaction = Some(source);
        return;
    }
    // Worker is idle and no compaction is in flight — dispatch immediately. The
    // worker runs it synchronously and emits events on the bus; it does not send
    // a `WorkerResult`, so `worker_active` is not set.
    *compaction_active = true;
    if ctx
        .worker_tx
        .send(WorkerCommand::RunCompaction { source })
        .await
        .is_err()
    {
        let _ = ctx
            .bus
            .compaction()
            .send(SystemEvent::CompactionFailed {
                source: "auto".to_string(),
                message: "Worker channel closed".to_string(),
            })
            .await;
    }
}

/// Handle a worker turn outcome: clear the active flag, drain queued blocking
/// requests, dispatch any queued compaction, and honor a pending quit.
///
/// Returns `true` when the loop should break (a quit was pending and the worker
/// is now idle).
pub(crate) async fn handle_worker_result<U, Se>(
    outcome: TurnOutcome,
    ctx: &mut OrchestrationContext<'_>,
    ui_request: &mut U,
    session: &mut Se,
    pending_compaction: &mut Option<String>,
    quit_pending: &mut bool,
) -> bool
where
    U: UiRequestHandler,
    Se: SessionHandler,
{
    session.handle_outcome(outcome, ctx).await;
    // Worker is now idle — drain queued blocking requests.
    ui_request.drain_queued(ctx).await;
    // If a compaction was queued while the worker was busy, run it now. The
    // worker runs it synchronously and emits events on the bus; it does not send
    // a `WorkerResult`, so `worker_active` is not set.
    if let Some(source) = pending_compaction.take()
        && ctx
            .worker_tx
            .send(WorkerCommand::RunCompaction { source })
            .await
            .is_err()
    {
        let _ = ctx
            .bus
            .compaction()
            .send(SystemEvent::CompactionFailed {
                source: "auto".to_string(),
                message: "Worker channel closed".to_string(),
            })
            .await;
    }
    // If a quit was requested while the worker was active and the worker is now
    // idle, exit the loop.
    *quit_pending && !*ctx.worker_active
}

/// A prompt that arrived while a blocking UI request was pending, paired with
/// the A2A task id that queued it (if any).
pub(crate) struct DeferredPrompt {
    pub(crate) text: String,
    pub(crate) task_id: Option<String>,
}

/// Submit a prompt that was deferred while a blocking UI request was pending.
///
/// The prompt is submitted only when the worker is idle, no blocking request
/// remains pending, and no quit is in flight; otherwise it stays deferred. A
/// prompt is never discarded, and a quit never starts a new turn.
pub(crate) async fn submit_deferred_prompt<S, U>(
    deferred_prompt: &mut Option<DeferredPrompt>,
    quit_pending: bool,
    slash: &mut S,
    ui_request: &U,
    ctx: &mut OrchestrationContext<'_>,
) where
    S: SlashHandler,
    U: UiRequestHandler,
{
    if quit_pending || *ctx.worker_active || ui_request.has_blocking_pending() {
        return;
    }
    let Some(entry) = deferred_prompt.take() else {
        return;
    };
    log::info!("submitting the prompt deferred while a blocking request was pending");
    slash.handle(entry.text, entry.task_id, ctx).await;
}

/// Handle an external (A2A) task cancellation.
pub(crate) async fn handle_external_cancel(task_id: String, ctx: &mut OrchestrationContext<'_>) {
    if ctx.active_external_task_id.as_deref() == Some(task_id.as_str()) {
        let _ = ctx.bus.cancel().send(CancelEvent::Requested).await;
    } else {
        *ctx.pending_external_cancel = Some(task_id);
    }
}

/// Reject an incoming A2A task that arrived while the worker was busy.
///
/// The server handler already transitioned the task `Submitted`→`Working`, so
/// leaving it untouched would strand the A2A caller. When no task store is
/// present the task is logged and dropped.
pub(crate) fn reject_busy_task(task_id: &str, ctx: &OrchestrationContext<'_>) {
    let Some(store) = ctx.task_store else {
        log::warn!(
            "A2A task {task_id} arrived while the worker was busy; no task store to reject it"
        );
        return;
    };
    if let Err(e) = store.reject_task(task_id, WORKER_BUSY_REASON) {
        log::warn!("failed to reject busy A2A task {task_id}: {e}");
    }
}

/// Reject the A2A task of a deferred prompt that a newer prompt replaced.
///
/// The server handler already transitioned the task `Submitted`→`Working`, so
/// leaving it untouched would strand the A2A caller until the send timeout. When
/// no task store is present the task is logged and dropped.
pub(crate) fn reject_superseded_task(task_id: &str, ctx: &OrchestrationContext<'_>) {
    let Some(store) = ctx.task_store else {
        log::warn!("A2A task {task_id} was superseded while deferred; no task store to reject it");
        return;
    };
    if let Err(e) = store.reject_task(task_id, DEFERRED_PROMPT_SUPERSEDED_REASON) {
        log::warn!("failed to reject superseded A2A task {task_id}: {e}");
    }
}

/// Await the next value from an optional channel, or never complete when the
/// channel is absent. Returns `None` when the channel is present but closed.
pub(crate) async fn recv_or_pending<T>(rx: &mut Option<mpsc::Receiver<T>>) -> Option<T> {
    match rx {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}
