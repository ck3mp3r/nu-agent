//! The orchestrator's event loop, its stage seam, and the single-turn entry
//! point. The loop owner composes the stages and drives this engine;
//! [`run_single_turn`] drives one turn without the loop.

use agnt_kernel::CompactionSource;
use nu_protocol::{LabeledError, Value};
use tokio::sync::mpsc;

use super::SourceChannels;
use crate::bus::RequestCancel;
use crate::conversation::turn::TurnEnd;
use crate::orchestrator::events::{OrchestratorEvent, WorkerCommand};
use crate::orchestrator::external::ExternalTaskStore;
use crate::orchestrator::handlers::{
    WORKER_BUSY_REASON, dispatch_pending_completions, dispatch_queued_external_prompt,
    submit_prompt,
};
use crate::orchestrator::stages::{
    OrchestrationContext, PermissionHandler, SessionHandler, SlashHandler, UiRequestHandler,
};
use crate::orchestrator::{
    CompletionBuffer, ExternalQueue, dispatch_compaction, handle_external_cancel,
    handle_worker_result, recv_or_pending, reject_external_task, submit_deferred_prompt,
};
use crate::protocol::contracts::CoreRuntime;
use agnt_kernel::bus::{ControlCommand, LifecycleEvent, ReceiveError};

/// Capacity of the worker command channel. The test harness builds its worker
/// channel from this constant, so a saturation test fills the real production
/// width.
pub(crate) const WORKER_CMD_CAPACITY: usize = 256;

pub async fn run_single_turn<R>(
    runtime: &mut R,
    task_id: Option<String>,
    prompt: String,
    context: Option<String>,
    span: nu_protocol::Span,
) -> Result<Value, LabeledError>
where
    R: CoreRuntime,
{
    // `task_id` is the external task this turn runs for, or `None` for a
    // prompt the user submitted. It rides the runtime call, so the executor's
    // spine bracket carries the attribution the caller holds in hand. The
    // write-back itself is the caller's: this seam runs the turn, it does not
    // transition a store.
    match runtime.execute_turn(prompt, context, span, task_id).await {
        // A stop is not an error for a one-shot turn: the turn's own response
        // is what the caller prints, exactly as before the end was typed.
        TurnEnd::Completed { value } | TurnEnd::Cancelled { value } => Ok(value),
        TurnEnd::Failed(failure) => Err(failure.error),
    }
}

/// The orchestrator event loop. Selects over the real source channels and
/// dispatches each event to the appropriate stage handler.
///
/// Generic over the four stage traits (DIP + ISP). The loop owns no stage state;
/// stages are passed in as `&mut` and mutated in place.
pub(crate) struct Stages<'a, S, P, U, Se> {
    pub slash: &'a mut S,
    pub permission: &'a mut P,
    pub ui_request: &'a mut U,
    pub session: &'a mut Se,
}

pub(crate) async fn run_orchestrator_loop<S, P, U, Se, E>(
    mut event_rx: mpsc::Receiver<OrchestratorEvent>,
    mut sources: SourceChannels<'_, E>,
    stages: Stages<'_, S, P, U, Se>,
    ctx: &mut OrchestrationContext<'_>,
) -> Result<(), LabeledError>
where
    S: SlashHandler,
    P: PermissionHandler,
    U: UiRequestHandler,
    Se: SessionHandler,
    E: ExternalTaskStore,
{
    let Stages {
        slash,
        permission,
        ui_request,
        session,
    } = stages;
    let mut pending_compaction: Option<CompactionSource> = None;
    let mut compaction_active = false;
    let mut quit_pending = false;
    // Completion events that arrived while the worker could not take a turn, in
    // arrival order and keyed by the remote task each reports. One is dispatched
    // per idle cycle so every completion gets its own turn, and a repeated
    // notification for the same remote task keeps one entry in the place it
    // first took.
    let mut pending_completions = CompletionBuffer::default();
    // The prompts that arrived from outside the surface, in arrival order, and
    // the cancel that waits for its task's turn to begin. The loop owns this
    // queue: it admits each arrival, holds a cancel until the turn it names
    // begins, dispatches one turn at a time, and hands each turn the id it is
    // attributed by.
    let mut external_queue = ExternalQueue::default();
    // A prompt submitted by the surface while a blocking UI request was pending.
    // It is submitted once the blocking request completes and the worker is
    // idle, so a prompt is never silently discarded.
    let mut deferred_prompt: Option<String> = None;

    // The task-cancel source is optional. When absent, select on a dummy
    // receiver that is never closed, so the `select!` arm always exists and the
    // loop does not spin.
    let (dummy_cancel_tx, dummy_cancel_rx) = mpsc::unbounded_channel::<String>();
    let mut task_cancel_rx = sources.task_cancel_rx.take().unwrap_or(dummy_cancel_rx);
    let _ = dummy_cancel_tx;

    // The external task and completion sources are optional. Each is polled
    // through a helper that awaits `pending()` forever when the channel is
    // absent, so the `select!` arms always exist and the loop never spins. When
    // a channel closes, the corresponding `Option` is set to `None` so the arm
    // stops polling it.
    let mut external_task_rx_opt = sources.external_task_rx.take();
    let mut external_completion_rx_opt = sources.external_completion_rx.take();
    // The rejection seam is a shared reference, so reading it here keeps it out
    // of every select arm's borrow.
    let external_task_store = sources.external_task_store;

    loop {
        tokio::select! {
            biased;

            recv = sources.lifecycle_rx.recv() => {
                match recv {
                    Ok(LifecycleEvent::CompactionCompleted { .. }
                    | LifecycleEvent::CompactionFailed { .. }) => {
                        compaction_active = false;
                        // Drain a queued compaction, if any.
                        if let Some(source) = pending_compaction.take() {
                            dispatch_compaction(
                                source,
                                ctx,
                                &mut pending_compaction,
                                &mut compaction_active,
                            )
                            .await;
                        }
                        // If a quit was requested while compaction was in
                        // flight and compaction is now idle, exit the loop once
                        // no buffered completion is still waiting for its turn;
                        // otherwise start the flush here, because nothing else
                        // would run it.
                        if quit_pending && !compaction_active {
                            if pending_completions.is_empty() {
                                break;
                            }
                            dispatch_pending_completions(
                                &mut pending_completions,
                                slash,
                                ui_request,
                                ctx,
                            )
                            .await;
                        }
                    }
                    // The runtime moved to another session — a surface switch or
                    // a peer attach. The catalog a picker refresh answers from
                    // lives here, so the omission follows the spine row both
                    // paths publish.
                    Ok(LifecycleEvent::SessionSwitched { to, .. }) => {
                        let catalog = ctx.catalog.clone().with_current_session(Some(to));
                        *ctx.catalog = catalog;
                    }
                    Ok(_) => {}
                    Err(ReceiveError::Closed) => break,
                    Err(_) => continue,
                }
            }

            ev = event_rx.recv() => {
                let Some(ev) = ev else { break };
                match ev {
                    OrchestratorEvent::PromptSubmitted { text } => {
                        // A submit from the surface belongs to no external task:
                        // the loop's own queue is the only road an attributed
                        // prompt travels. The prompt runs on its own turn.
                        submit_prompt(text, slash, ui_request, &mut deferred_prompt, ctx).await;
                    }
                    OrchestratorEvent::PermissionDecision { decision } => {
                        permission.handle(decision, ctx).await;
                    }
                    OrchestratorEvent::UiRequest(req) => {
                        ui_request.handle_incoming(req, ctx).await;
                    }
                    OrchestratorEvent::CancelRequested => {
                        let _ = ctx.cancel.request_cancel().await;
                    }
                    OrchestratorEvent::Quit => {
                        if *ctx.worker_active {
                            quit_pending = true;
                            let _ = ctx.cancel.request_cancel().await;
                            continue;
                        }
                        if ui_request.has_pending() {
                            quit_pending = true;
                            continue;
                        }
                        if pending_compaction.is_some() {
                            quit_pending = true;
                            continue;
                        }
                        if compaction_active {
                            quit_pending = true;
                            continue;
                        }
                        if !pending_completions.is_empty() {
                            // Quit does not abandon a buffered completion. Nothing
                            // else would run the flush, so start its first turn
                            // here and exit when the buffer is empty.
                            quit_pending = true;
                            dispatch_pending_completions(
                                &mut pending_completions,
                                slash,
                                ui_request,
                                ctx,
                            )
                            .await;
                            continue;
                        }
                        break;
                    }
                    OrchestratorEvent::FatalError(e) => {
                        return Err(e);
                    }
                }
            }

            outcome = sources.worker_result_rx.recv() => {
                let Some(outcome) = outcome else { break };
                if handle_worker_result(
                    outcome,
                    ctx,
                    ui_request,
                    session,
                    &mut pending_compaction,
                    &mut quit_pending,
                    pending_completions.is_empty(),
                )
                .await
                {
                    break;
                }
                // The turn that was in flight is over, so the queue is free to
                // reach the next prompt it holds.
                external_queue.finish_turn();
                // The worker is now idle. Submit a prompt that was deferred
                // while a blocking request was pending, dispatch the completion
                // that has waited longest, then reach the prompts the loop holds
                // for a turn. That statement order IS the per-cycle priority: one
                // turn per idle cycle, and each stage's guard keeps the later
                // ones out while a turn is running.
                submit_deferred_prompt(&mut deferred_prompt, quit_pending, slash, ui_request, ctx).await;
                dispatch_pending_completions(&mut pending_completions, slash, ui_request, ctx).await;
                dispatch_queued_external_prompt(
                    &mut external_queue,
                    quit_pending,
                    slash,
                    ui_request,
                    ctx,
                )
                .await;
            }

            resp = sources.blocking_response_rx.recv() => {
                let Some(resp) = resp else { break };
                ui_request.handle_blocking_response(resp, ctx).await;
                // The response released the blocking slot, so a switch that
                // queued behind the turn applies now — in queue order, before
                // any prompt starts a turn and takes the slot again.
                ui_request.drain_queued(ctx).await;
                // The blocking request is done: submit a prompt that was
                // deferred while it was pending, dispatch the completion that
                // has waited longest, then reach the prompts the loop holds for a
                // turn.
                submit_deferred_prompt(&mut deferred_prompt, quit_pending, slash, ui_request, ctx).await;
                dispatch_pending_completions(&mut pending_completions, slash, ui_request, ctx).await;
                dispatch_queued_external_prompt(
                    &mut external_queue,
                    quit_pending,
                    slash,
                    ui_request,
                    ctx,
                )
                .await;
                // A quit may exit only once every buffered completion has run its
                // turn and no turn is in flight. The turn this arm may just have
                // started ends in the worker-result arm, which finishes the flush
                // and exits when the buffer is empty.
                if quit_pending
                    && !ui_request.has_pending()
                    && pending_completions.is_empty()
                    && !*ctx.worker_active
                {
                    break;
                }
            }

            resp = sources.concurrent_response_rx.recv() => {
                let Some(resp) = resp else { break };
                ui_request.handle_concurrent_response(resp, ctx).await;
                // A quit defers on a buffered completion here too: the flush
                // needs this response to be the thing that frees it, so start the
                // next flush turn and exit only when the buffer is empty and no
                // turn is in flight.
                if quit_pending
                    && !ui_request.has_pending()
                    && pending_completions.is_empty()
                    && !*ctx.worker_active
                {
                    break;
                }
                if quit_pending {
                    dispatch_pending_completions(&mut pending_completions, slash, ui_request, ctx).await;
                }
            }

            recv = sources.control_rx.recv() => {
                match recv {
                    // Owned by the loop: the conversation asked for a sweep.
                    Ok(ControlCommand::CompactionRequested { source }) => {
                        dispatch_compaction(source, ctx, &mut pending_compaction, &mut compaction_active).await;
                    }
                    // Owned by the loop: a prompt arrived from outside the
                    // surface. The boundary that accepted it already published
                    // the row the surface draws — the router, where the
                    // runtime's identities are reachable. The loop owns the
                    // admit, the repeat-arrival refusal, and the queueing.
                    Ok(ControlCommand::EnqueueExternalPrompt { task_id, prompt }) => {
                        if !external_queue.admit(task_id.clone(), prompt.clone()) {
                            log::warn!(
                                "external task {task_id} is already queued or running; ignoring the repeat arrival"
                            );
                            continue;
                        }
                        dispatch_queued_external_prompt(
                            &mut external_queue,
                            quit_pending,
                            slash,
                            ui_request,
                            ctx,
                        )
                        .await;
                    }
                    // Owned by the loop: the surface answered the open card.
                    Ok(ControlCommand::PermissionDecided {
                        request_id,
                        decision,
                    }) => {
                        permission
                            .handle(
                                crate::protocol::event::PermissionDecisionSubmission {
                                    request_id: request_id.as_str().to_string(),
                                    decision,
                                },
                                ctx,
                            )
                            .await;
                    }
                    // Owned by the surface: a panel request the loop has no
                    // business acting on. Ignored here by design.
                    Ok(ControlCommand::SharedUiAction { .. }) => {}
                    Err(ReceiveError::Closed) => break,
                    Err(_) => continue,
                }
            }

            task_id = task_cancel_rx.recv() => {
                let Some(task_id) = task_id else { break };
                handle_external_cancel(task_id, &mut external_queue, ctx).await;
            }
            incoming = recv_or_pending(&mut external_task_rx_opt) => {
                let Some(incoming) = incoming else { break };
                // A task that arrives while a turn runs is ACCEPTED: its prompt
                // stacks in the loop's queue behind the running turn and its own
                // turn runs when the queue reaches it, one task at a time.
                //
                // The context id is already resolved at the boundary that
                // accepted the task, so the loop attaches the session it names.
                // The worker command channel is bounded: awaiting capacity here
                // would park the loop, so a full channel rejects the task and its
                // caller reaches a terminal state.
                let task_id = incoming.task_id;
                match ctx.worker_tx.try_send(WorkerCommand::AttachA2aContext {
                    context_id: incoming.context_id,
                    prompt: incoming.prompt,
                    task_id: task_id.clone(),
                }) {
                    // The router attaches the session and publishes the prompt on
                    // the control concern carrying this id, so the loop's queue
                    // takes the arrival and attributes the turn to this task.
                    Ok(()) => {}
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        let _ = reject_external_task(
                            &task_id,
                            external_task_store,
                            WORKER_BUSY_REASON,
                        );
                    }
                    Err(mpsc::error::TrySendError::Closed(_)) => {
                        log::warn!(
                            "worker channel closed; dropping external task {task_id}"
                        );
                    }
                }
            }

            event = recv_or_pending(&mut external_completion_rx_opt) => {
                let Some(event) = event else { break };
                // A completion holds in the buffer until the worker can take its
                // turn instead of being dropped. The entry keeps the place its
                // remote task first took, so a repeated notification for the same
                // remote task updates it without reordering the line.
                pending_completions.push(event);
                // While the worker is busy the next idle cycle dispatches it; while
                // a quit flushes the buffer this arrival joins that flush.
                if *ctx.worker_active {
                    continue;
                }
                dispatch_pending_completions(&mut pending_completions, slash, ui_request, ctx).await;
            }
        }
    }
    // Every `break` above lands here. The graceful quit paths empty the buffer
    // before they exit, so a completion still held means a source channel closed
    // and the owner is tearing down: no further turn can run for it. Name each
    // abandoned remote task at warn, the only record the loop can leave.
    for task_id in pending_completions.task_ids() {
        log::warn!(
            "abandoning the buffered completion for remote task {task_id}: the loop is shutting down"
        );
    }
    Ok(())
}
