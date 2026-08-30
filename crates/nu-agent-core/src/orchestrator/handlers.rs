//! Handler routines for the orchestrator loop: compaction dispatch, worker
//! result handling, external (A2A) prompt handling, and optional-channel awaits.

use tokio::sync::mpsc;

use crate::bus::{CancelEvent, CompactionEvent, TurnEvent};
use crate::orchestrator::stages::{OrchestrationContext, SessionHandler, UiRequestHandler};
use crate::orchestrator::turn_outcome::TurnOutcome;
use crate::orchestrator::{UiStateEvent, WorkerCommand};

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
            .send(CompactionEvent::Failed {
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
            .send(CompactionEvent::Failed {
                source: "auto".to_string(),
                message: "Worker channel closed".to_string(),
            })
            .await;
    }
    // If a quit was requested while the worker was active and the worker is now
    // idle, exit the loop.
    *quit_pending && !*ctx.worker_active
}

/// Handle an external (A2A) prompt: dispatch a turn if the worker is idle.
pub(crate) async fn handle_external_prompt(
    prompt: String,
    task_id: String,
    ctx: &mut OrchestrationContext<'_>,
) {
    if !*ctx.worker_active {
        let _ = ctx
            .bus
            .ui_state()
            .send(UiStateEvent::DisplayIncomingMessage(prompt.clone()))
            .await;
        *ctx.active_external_prompt = Some(prompt.clone());
        *ctx.active_external_task_id = Some(task_id.clone());
        if ctx.pending_external_cancel.as_deref() == Some(task_id.as_str()) {
            *ctx.pending_external_cancel = None;
            let _ = ctx.bus.cancel().send(CancelEvent::Requested).await;
        }
        let _ = ctx
            .bus
            .turn()
            .send(TurnEvent::Started {
                prompt: prompt.clone(),
                task_id: Some(task_id),
            })
            .await;
        let _ = ctx
            .worker_tx
            .send(WorkerCommand::ExecuteTurn {
                prompt,
                span: ctx.span,
            })
            .await;
        *ctx.worker_active = true;
    }
}

/// Handle an external (A2A) task cancellation.
pub(crate) async fn handle_external_cancel(task_id: String, ctx: &mut OrchestrationContext<'_>) {
    if ctx.active_external_task_id.as_deref() == Some(task_id.as_str()) {
        let _ = ctx.bus.cancel().send(CancelEvent::Requested).await;
    } else {
        *ctx.pending_external_cancel = Some(task_id);
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
