//! Handler routines for the orchestrator loop: compaction dispatch, worker
//! result handling, external (A2A) cancellation, and optional-channel awaits.

use std::sync::Arc;
use std::time::Duration;

use agnt_kernel::CompactionSource;
use agnt_kernel::{PromptRecord, PromptStamps, SessionRecord};
use tokio::sync::mpsc;

use crate::bus::RequestCancel;
use crate::orchestrator::events::{UiRequestResponse, WorkerCommand};
use crate::orchestrator::external::ExternalTaskStore;
use crate::orchestrator::r#loop::{
    CancelDisposition, CompletionBuffer, ExternalQueue, SourceChannels,
};
use crate::orchestrator::stages::{
    OrchestrationContext, SessionHandler, SlashHandler, UiRequestHandler,
};
use crate::orchestrator::turn_outcome::TurnResult;
use agnt_kernel::bus::LifecycleEvent;

/// Reason recorded on a task rejected because the user cancelled.
pub(crate) const CANCELLED_BY_USER_REASON: &str = "Cancelled by user";

/// Reason recorded on a task rejected or failed because the worker was busy.
pub(crate) const WORKER_BUSY_REASON: &str = "Worker busy: another turn is already running";

/// How long quit waits for the UI responses already on their way.
///
/// Quit abandons what it cannot answer, and abandoning a response that is a
/// moment away would drop the effect the user just asked for. The wait is
/// bounded, so a response that never arrives cannot defer the exit.
const QUIT_SETTLE: Duration = Duration::from_millis(50);

/// The transcript rewrites the loop holds for the worker: a manual compaction
/// and an uncompaction, each with the source that asked for it.
///
/// The two share the compaction-active slot, so at most one runs at a time and
/// a quit is deferred for both. Each slot holds one request: a newer request
/// replaces the queued one.
#[derive(Debug, Default)]
pub(crate) struct PendingRewrites {
    /// The compaction waiting for the worker.
    pub(crate) compaction: Option<CompactionSource>,
    /// The uncompaction waiting for the worker.
    pub(crate) uncompaction: Option<CompactionSource>,
}

/// Dispatch a compaction command to the worker, or queue it if a compaction is
/// already in flight or the worker is busy running a turn.
///
/// Manual only: `/compact` is the one source that reaches this path. The
/// automatic ladder completes its compaction on the turn itself.
pub(crate) async fn dispatch_compaction(
    source: CompactionSource,
    ctx: &mut OrchestrationContext<'_>,
    pending: &mut PendingRewrites,
    compaction_active: &mut bool,
) {
    if *compaction_active || *ctx.worker_active {
        // A compaction is already in flight or the worker is busy running a
        // turn — queue one pending compaction. A newer request replaces the
        // queued one.
        pending.compaction = Some(source);
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
        let _ = ctx.lifecycle.publish(LifecycleEvent::CompactionFailed {
            source,
            message: "Worker channel closed".to_string(),
        });
    }
}

/// Dispatch an uncompaction command to the worker, or queue it if a compaction
/// or uncompaction is already in flight or the worker is busy running a turn.
///
/// Manual only: `/uncompact` is the one source that reaches this path. The
/// uncompaction shares the compaction-active slot: one transcript rewrite runs
/// at a time, so a quit is deferred by the same guard for both.
pub(crate) async fn dispatch_uncompact(
    source: CompactionSource,
    ctx: &mut OrchestrationContext<'_>,
    pending: &mut PendingRewrites,
    compaction_active: &mut bool,
) {
    if *compaction_active || *ctx.worker_active {
        // A transcript rewrite is in flight or the worker is busy running a
        // turn — queue one pending uncompaction. A newer request replaces the
        // queued one.
        pending.uncompaction = Some(source);
        return;
    }
    *compaction_active = true;
    if ctx
        .worker_tx
        .send(WorkerCommand::RunUncompact { source })
        .await
        .is_err()
    {
        let _ = ctx.lifecycle.publish(LifecycleEvent::UncompactFailed {
            source,
            message: "Worker channel closed".to_string(),
        });
    }
}

/// Apply the UI responses that arrive while a quit settles, then return.
///
/// Only the two response lanes are read: this runs inside the quit arm, so it
/// starts no turn and dispatches no queued effect. A response that arrives
/// within [`QUIT_SETTLE`] lands its effect; whatever is still unanswered when
/// the wait ends is abandoned by the caller, so quit cannot hang on it.
pub(crate) async fn settle_ui_responses<U, S>(
    ui_request: &mut U,
    sources: &mut SourceChannels<'_, S>,
    ctx: &mut OrchestrationContext<'_>,
) where
    U: UiRequestHandler,
    S: ExternalTaskStore,
{
    let _ = tokio::time::timeout(QUIT_SETTLE, async {
        while ui_request.has_pending() {
            tokio::select! {
                Some(response) = sources.blocking_response_rx.recv() => {
                    ui_request.handle_blocking_response(response, ctx).await;
                }
                Some(response) = sources.concurrent_response_rx.recv() => {
                    ui_request.handle_concurrent_response(response, ctx).await;
                }
                else => break,
            }
        }
    })
    .await;
}

/// Fold every UI response already sitting on the two lanes, without waiting.
///
/// The quit arm calls this after it abandons the entries that never answered:
/// each abandonment published that entry's substitute answer, so folding here is
/// what releases the surface — the modal, the picker, the toggle — before the
/// loop breaks. Nothing is awaited: an empty lane is simply empty, and the
/// substitute published an instant ago is already queued.
pub(crate) async fn fold_ui_responses<U>(
    ui_request: &mut U,
    blocking_response_rx: &mut mpsc::Receiver<UiRequestResponse>,
    concurrent_response_rx: &mut mpsc::Receiver<UiRequestResponse>,
    ctx: &mut OrchestrationContext<'_>,
) where
    U: UiRequestHandler,
{
    while let Ok(response) = blocking_response_rx.try_recv() {
        ui_request.handle_blocking_response(response, ctx).await;
    }
    while let Ok(response) = concurrent_response_rx.try_recv() {
        ui_request.handle_concurrent_response(response, ctx).await;
    }
}

/// Handle a worker turn outcome: clear the active flag, drain queued blocking
/// requests, dispatch any queued compaction or uncompaction, and honor a
/// pending quit.
///
/// Returns `true` when the loop should break: a quit was pending, the worker is
/// now idle, and no buffered completion waits for its turn.
pub(crate) async fn handle_worker_result<U, Se>(
    outcome: TurnResult,
    ctx: &mut OrchestrationContext<'_>,
    ui_request: &mut U,
    session: &mut Se,
    pending: &mut PendingRewrites,
    quit_pending: &mut bool,
    completions_empty: bool,
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
    if let Some(source) = pending.compaction.take()
        && ctx
            .worker_tx
            .send(WorkerCommand::RunCompaction { source })
            .await
            .is_err()
    {
        let _ = ctx.lifecycle.publish(LifecycleEvent::CompactionFailed {
            source,
            message: "Worker channel closed".to_string(),
        });
    }
    // An uncompaction queued behind a turn is dispatched the same way.
    if let Some(source) = pending.uncompaction.take()
        && ctx
            .worker_tx
            .send(WorkerCommand::RunUncompact { source })
            .await
            .is_err()
    {
        let _ = ctx.lifecycle.publish(LifecycleEvent::UncompactFailed {
            source,
            message: "Worker channel closed".to_string(),
        });
    }
    // If a quit was requested while the worker was active, the worker is now
    // idle, and no completion is still waiting for its turn, exit the loop. A
    // buffered completion defers the exit: it is an intake the loop promised to
    // run, so quitting must not abandon it.
    *quit_pending && !*ctx.worker_active && completions_empty
}

/// Submit a prompt from the surface, now or once a blocking request completes.
///
/// A prompt is never discarded. A prompt that arrives while a blocking request
/// is pending is held and submitted when the request completes and the worker is
/// idle; a second one replaces it. It carries no attribution: a prompt submitted
/// by the surface belongs to no external task.
pub(crate) async fn submit_prompt<S, U>(
    text: String,
    slash: &mut S,
    ui_request: &U,
    deferred_prompt: &mut Option<String>,
    ctx: &mut OrchestrationContext<'_>,
) where
    S: SlashHandler,
    U: UiRequestHandler,
{
    if ui_request.has_blocking_pending() {
        log::warn!("prompt deferred while a blocking request is pending");
        *deferred_prompt = Some(text);
        return;
    }
    slash.handle(text, None, ctx).await;
}

/// Submit the prompt deferred while a blocking UI request was pending.
///
/// The prompt is submitted only when the worker is idle, no blocking request
/// remains pending, and no quit is in flight; otherwise it stays deferred. A
/// prompt is never discarded, and a quit never starts a new turn.
pub(crate) async fn submit_deferred_prompt<S, U>(
    deferred_prompt: &mut Option<String>,
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
    let Some(text) = deferred_prompt.take() else {
        return;
    };
    log::info!("submitting the prompt deferred while a blocking request was pending");
    slash.handle(text, None, ctx).await;
}

/// Dispatch the completion that has waited longest, when the worker is free.
///
/// This is the one site that submits a buffered completion. The buffer drains in
/// arrival order, one turn per idle cycle, and each turn is unattributed: a
/// completion reports a REMOTE task, never one a caller handed this agent. The
/// loop is this completion's accepting boundary, so it also publishes the prompt
/// row the surface draws for the turn.
///
/// The guard is the same one the queued external prompt honors, minus the quit:
/// a running turn and an open blocking request both defer the dispatch. While a
/// quit is pending the loop is flushing this buffer, so the quit itself no longer
/// defers — nothing else would run the flush — and the surface prompt and the
/// external queue keep their own quit early-returns, so no other turn starts
/// during the quit window.
pub(crate) async fn dispatch_pending_completions<S, U>(
    completions: &mut CompletionBuffer,
    slash: &mut S,
    ui_request: &U,
    ctx: &mut OrchestrationContext<'_>,
) where
    S: SlashHandler,
    U: UiRequestHandler,
{
    if *ctx.worker_active || ui_request.has_blocking_pending() {
        return;
    }
    let Some(completion) = completions.take_next() else {
        return;
    };
    // The turn this completion queues carries no attribution: its remote task
    // id keys the wait, it never becomes an attribution. No other boundary sees
    // this arrival, so the row the surface draws is published here, with the
    // instant the loop dispatched it and no identity the loop cannot reach.
    let _ = ctx.stream.publish(SessionRecord::Prompt(PromptRecord {
        text: completion.prompt.clone(),
        stamps: PromptStamps {
            millis: Some(chrono::Utc::now().timestamp_millis()),
            model: None,
            agent: None,
        },
        task_id: None,
    }));
    slash.handle(completion.prompt, None, ctx).await;
}

/// Dispatch the next prompt the loop holds for a turn, when the worker is free.
///
/// One turn runs at a time: the queue drains in arrival order, and an entry is
/// dispatched only when no turn and no blocking request is pending. A cancel
/// that arrived before the task's turn began is honoured the moment it begins.
pub(crate) async fn dispatch_queued_external_prompt<S, U>(
    queue: &mut ExternalQueue,
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
    let Some(entry) = queue.take_next() else {
        return;
    };
    slash
        .handle(entry.prompt, Some(entry.task_id.clone()), ctx)
        .await;
    if !*ctx.worker_active {
        // The prompt ran no turn (a slash command): nothing is in flight for it,
        // and the queue is free to reach the next prompt.
        return;
    }
    // The turn begins: the queue binds it, and a cancel that waited for this
    // task is honoured here, once, by the turn it named.
    if queue.begin_turn(entry.task_id) {
        let _ = ctx.cancel.request_cancel().await;
    }
}

/// Handle an external (A2A) task cancellation.
///
/// The queue owns where the cancel lands: it stops the task's in-flight turn
/// now, or holds the cancel until that task's own turn begins.
pub(crate) async fn handle_external_cancel(
    task_id: String,
    queue: &mut ExternalQueue,
    ctx: &mut OrchestrationContext<'_>,
) {
    if queue.cancel(task_id) == CancelDisposition::StopsInFlight {
        let _ = ctx.cancel.request_cancel().await;
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

/// Reject an external task through the configured store.
///
/// The task's caller already transitioned it to a running state, so leaving it
/// untouched would strand that caller. When no store is present the task is
/// logged and dropped. A store failure is logged and reported, so its caller can
/// keep the loop running.
pub(crate) fn reject_external_task<E: ExternalTaskStore>(
    task_id: &str,
    store: Option<&Arc<E>>,
    reason: &str,
) -> Result<(), String> {
    let Some(store) = store else {
        log::warn!(
            "external task {task_id} cannot run ({reason}); no external task store to reject it"
        );
        return Ok(());
    };
    if let Err(e) = store.reject_task(task_id, reason) {
        log::warn!("failed to reject external task {task_id}: {e}");
        return Err(e);
    }
    Ok(())
}
