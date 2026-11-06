//! The orchestrator's event loop, its stage seam, and the single-turn entry
//! point. The loop owner composes the stages and drives this engine;
//! [`run_single_turn`] drives one turn without the loop.

use std::collections::VecDeque;

use agnt_a2a::A2aCompletionEvent;
use agnt_kernel::CompactionSource;
use nu_protocol::{LabeledError, Value};
use tokio::sync::mpsc;

use super::SourceChannels;
use crate::bus::RequestCancel;
use crate::orchestrator::stages::{
    OrchestrationContext, PermissionHandler, SessionHandler, SlashHandler, UiRequestHandler,
};
use crate::orchestrator::{
    DeferredPrompt, OrchestratorEvent, UiStateEvent, WorkerCommand, dispatch_compaction,
    handle_external_cancel, handle_worker_result, recv_or_pending, reject_busy_task,
    reject_superseded_task, submit_deferred_prompt,
};
use crate::protocol::contracts::CoreRuntime;
use crate::protocol::events::SystemEvent;
use agnt_kernel::bus::{ChannelError, ExternalEvent, TurnEvent, TurnTx};

/// Capacity of the worker command channel. The test harness builds its worker
/// channel from this constant, so a saturation test fills the real production
/// width.
pub(crate) const WORKER_CMD_CAPACITY: usize = 256;

pub async fn run_single_turn<R>(
    runtime: &mut R,
    turn: &TurnTx,
    prompt: String,
    context: Option<String>,
    span: nu_protocol::Span,
) -> Result<Value, LabeledError>
where
    R: CoreRuntime,
{
    // Control-plane turn-start event: published on the turn channel, not
    // rendered. The single-turn path has no A2A attribution at dispatch — the
    // task store is updated after the turn — so the attribution is always
    // `None`.
    let _ = turn
        .send(TurnEvent::Started {
            prompt: prompt.clone(),
            task_id: None,
        })
        .await;
    runtime.execute_turn(prompt, context, span).await
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

pub(crate) async fn run_orchestrator_loop<S, P, U, Se>(
    mut event_rx: mpsc::Receiver<OrchestratorEvent>,
    mut sources: SourceChannels<'_>,
    stages: Stages<'_, S, P, U, Se>,
    ctx: &mut OrchestrationContext<'_>,
) -> Result<(), LabeledError>
where
    S: SlashHandler,
    P: PermissionHandler,
    U: UiRequestHandler,
    Se: SessionHandler,
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
    // Completion events that arrived while the worker was busy. Each is drained
    // one per idle cycle so every completion gets its own turn.
    let mut pending_completion_events: VecDeque<A2aCompletionEvent> = VecDeque::new();
    // A prompt that arrived while a blocking UI request was pending, paired
    // with the A2A task id that queued it. It is submitted once the blocking
    // request completes and the worker is idle, so a prompt is never silently
    // discarded and its task attribution travels with it.
    let mut deferred_prompt: Option<DeferredPrompt> = None;

    // The task-cancel source is optional. When absent, select on a dummy
    // receiver that is never closed, so the `select!` arm always exists and the
    // loop does not spin.
    let (dummy_cancel_tx, dummy_cancel_rx) = mpsc::unbounded_channel::<String>();
    let mut task_cancel_rx = sources.task_cancel_rx.take().unwrap_or(dummy_cancel_rx);
    let _ = dummy_cancel_tx;

    // The incoming A2A task and completion-event sources are optional. Each is
    // polled through a helper that awaits `pending()` forever when the channel
    // is absent, so the `select!` arms always exist and the loop never spins.
    // When a channel closes, the corresponding `Option` is set to `None` so the
    // arm stops polling it.
    let mut a2a_task_rx_opt = sources.a2a_task_rx.take();
    let mut a2a_completion_rx_opt = sources.a2a_completion_rx.take();

    loop {
        tokio::select! {
            biased;

            recv = sources.external_rx.recv() => {
                match recv {
                    Ok(ExternalEvent::PromptReceived { prompt, task_id, context_id }) => {
                        // The prompt enters the TUI prompt queue. The id is held
                        // in `pending_a2a_task_id` until the matching
                        // `PromptSubmitted` fires, which pairs it with the prompt
                        // it belongs to.
                        *ctx.pending_a2a_task_id = Some(task_id);
                        match context_id {
                            Some(context_id) => {
                                let _ = ctx
                                    .worker_tx
                                    .send(WorkerCommand::AttachA2aContext {
                                        context_id,
                                        prompt,
                                    })
                                    .await;
                            }
                            None => {
                                let _ = ctx
                                    .ui_state
                                    .send(UiStateEvent::EnqueueExternalPrompt { text: prompt })
                                    .await;
                            }
                        }
                    }
                    Err(ChannelError::Closed) => break,
                    Err(ChannelError::Lagged { .. }) => continue,
                    Err(_) => continue,
                }
            }

            ev = event_rx.recv() => {
                let Some(ev) = ev else { break };
                match ev {
                    OrchestratorEvent::PromptSubmitted { text } => {
                        if ui_request.has_blocking_pending() {
                            log::warn!("prompt deferred while a blocking request is pending");
                            let entry = DeferredPrompt {
                                text,
                                task_id: ctx.pending_a2a_task_id.take(),
                            };
                            if let Some(victim) = deferred_prompt.replace(entry) {
                                log::warn!(
                                    "a second prompt arrived while one was deferred; replacing it"
                                );
                                // The victim's A2A task is already `Working`; reject
                                // it so its caller reaches a terminal state.
                                if let Some(task_id) = victim.task_id {
                                    reject_superseded_task(&task_id, ctx);
                                }
                            }
                        } else {
                            let task_id = ctx.pending_a2a_task_id.take();
                            slash.handle(text, task_id, ctx).await;
                        }
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
                        break;
                    }
                    OrchestratorEvent::FatalError(e) => {
                        return Err(e);
                    }
                }
            }

            outcome = sources.worker_result_rx.recv() => {
                let Some(outcome) = outcome else { break };
                if handle_worker_result(outcome, ctx, ui_request, session, &mut pending_compaction, &mut quit_pending).await {
                    break;
                }
                // The worker is now idle: submit a prompt that was deferred
                // while a blocking request was pending.
                submit_deferred_prompt(&mut deferred_prompt, quit_pending, slash, ui_request, ctx).await;
                // The worker is now idle. Drain one buffered completion event so
                // it gets its own turn; the next event waits for the next idle
                // cycle. `continue` returns to `select!`, so new events can
                // interleave between buffered completions.
                if !*ctx.worker_active
                    && let Some(event) = pending_completion_events.pop_front()
                {
                    // A completion event is a notification about a REMOTE task,
                    // so it carries no local task attribution.
                    let prompt = event.to_prompt();
                    let _ = ctx
                        .ui_state
                        .send(UiStateEvent::EnqueueExternalPrompt { text: prompt })
                        .await;
                    continue;
                }
            }

            resp = sources.blocking_response_rx.recv() => {
                let Some(resp) = resp else { break };
                ui_request.handle_blocking_response(resp, ctx).await;
                // The blocking request is done: submit a prompt that was
                // deferred while it was pending.
                submit_deferred_prompt(&mut deferred_prompt, quit_pending, slash, ui_request, ctx).await;
                if quit_pending && !ui_request.has_pending() {
                    break;
                }
            }

            resp = sources.concurrent_response_rx.recv() => {
                let Some(resp) = resp else { break };
                ui_request.handle_concurrent_response(resp, ctx).await;
                if quit_pending && !ui_request.has_pending() {
                    break;
                }
            }

            recv = sources.compaction_rx.recv() => {
                match recv {
                    Ok(SystemEvent::CompactionRequested { source }) => {
                        dispatch_compaction(source, ctx, &mut pending_compaction, &mut compaction_active).await;
                    }
                    Ok(SystemEvent::CompactionCompleted { .. } | SystemEvent::CompactionFailed { .. }) => {
                        compaction_active = false;
                        // Drain a queued compaction, if any.
                        if let Some(source) = pending_compaction.take() {
                            dispatch_compaction(source, ctx, &mut pending_compaction, &mut compaction_active).await;
                        }
                        // If a quit was requested while compaction was in
                        // flight and compaction is now idle, exit the loop.
                        if quit_pending && !compaction_active {
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(ChannelError::Closed) => break,
                    Err(ChannelError::Lagged { .. }) => continue,
                    Err(_) => continue,
                }
            }

            task_id = task_cancel_rx.recv() => {
                let Some(task_id) = task_id else { break };
                handle_external_cancel(task_id, ctx).await;
            }

            incoming = recv_or_pending(&mut a2a_task_rx_opt) => {
                let Some(mut incoming) = incoming else { break };
                // The server handler already transitioned the task
                // Submitted→Working. If the worker is busy, no turn can run for
                // it, so reject it instead of leaving it stuck in Working.
                if *ctx.worker_active {
                    reject_busy_task(&incoming.task_id, ctx);
                    continue;
                }
                // An absent `contextId` means "start a fresh session" (A2A spec
                // §3.4.1). Mint a UUID so the client receives a context handle
                // it can resume with on a follow-up task.
                let context_id = agnt_a2a::resolve_context_id(incoming.context_id.take());
                incoming.context_id = Some(context_id.clone());
                let prompt = incoming.to_prompt();
                // The worker command channel is bounded. Awaiting capacity here
                // would park the loop — and with it the TUI event channel —
                // whenever the worker is saturated. Compaction leaves
                // `worker_active` false while the worker is busy, so this arm is
                // reached with a full channel and the awaited send froze input
                // until compaction ended. Reject the task instead so its caller
                // reaches a terminal state.
                match ctx.worker_tx.try_send(WorkerCommand::AttachA2aContext {
                    context_id,
                    prompt,
                }) {
                    Ok(()) => {
                        // The router attaches the session and enqueues the prompt
                        // on the UI bus; the loop pairs this task id with the
                        // resulting `PromptSubmitted` so the turn is attributed
                        // to this task.
                        *ctx.pending_a2a_task_id = Some(incoming.task_id);
                    }
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        reject_busy_task(&incoming.task_id, ctx);
                    }
                    Err(mpsc::error::TrySendError::Closed(_)) => {
                        log::warn!(
                            "worker channel closed; dropping A2A task {}",
                            incoming.task_id
                        );
                    }
                }
            }

            event = recv_or_pending(&mut a2a_completion_rx_opt) => {
                let Some(event) = event else { break };
                // A completion event is a prompt into the orchestrator's own
                // session. If the worker is busy, the prompt cannot run yet, so
                // buffer the event and drain it once the worker is idle.
                if *ctx.worker_active {
                    pending_completion_events.push_back(event);
                    continue;
                }
                let prompt = event.to_prompt();
                // The remote `contextId` is metadata only, so no session attach
                // is requested here. The event is a notification about a REMOTE
                // task, so it carries no local task attribution.
                let _ = ctx
                    .ui_state
                    .send(UiStateEvent::EnqueueExternalPrompt { text: prompt })
                    .await;
            }
        }
    }
    Ok(())
}
