//! The interactive loop, the orchestrator's event loop, and single-turn entry
//! points. These functions wire the worker, the stages, the bus channels, and
//! the render-loop together and drive the orchestrator loop.

use nu_agent_a2a::{A2aCompletionEvent, IncomingTask, Part};
use nu_protocol::{LabeledError, Value};
use tokio::sync::mpsc;

use crate::bus::{
    Bus, ChannelError, CompactionEvent, CompactionRx, ExternalEvent, ExternalRx, SessionEvent,
};
use crate::orchestrator::router::CommandRouter;
use crate::orchestrator::stages::{
    OrchestrationContext, PermissionHandler, SessionHandler, SlashHandler, UiRequestHandler,
    permission::PermissionStage, session::SessionStage, slash::SlashStage,
    ui_request::UiRequestStage,
};
use crate::orchestrator::turn_outcome::TurnOutcome;
use crate::orchestrator::{
    InteractiveLoopConfig, OrchestratorEvent, UiRequestResponse, UiStateEvent, WorkerCommand,
    dispatch_compaction, handle_external_cancel, handle_external_prompt, handle_worker_result,
    recv_or_pending,
};
use crate::protocol::{
    contracts::CoreRuntime,
    mcp_management::McpManagement,
    model_switching::ModelSwitching,
    session_management::{SessionPersistence, SessionState},
};

pub(crate) async fn run_interactive_loop_impl<R, F>(
    mut runtime: R,
    config: InteractiveLoopConfig<F>,
) -> (R, Result<Value, LabeledError>)
where
    R: CoreRuntime
        + McpManagement
        + ModelSwitching
        + SessionState
        + SessionPersistence
        + Send
        + 'static,
    F: FnOnce(mpsc::Sender<OrchestratorEvent>) + Send + 'static,
{
    let InteractiveLoopConfig {
        span,
        interactive_pending,
        task_cancel_rx,
        a2a_task_rx,
        a2a_completion_rx,
        bus,
        hydration,
        on_agent_switch,
        spawn_render_loop,
    } = config;

    if let Some(hydration) = hydration {
        let _ = bus
            .ui_state()
            .send(UiStateEvent::HydrateTranscript {
                messages: hydration.messages,
                last_total_tokens: hydration.last_total_tokens,
            })
            .await;
        runtime.seed_last_total_tokens(hydration.last_total_tokens);
        let _ = bus
            .session()
            .send(SessionEvent::Started {
                session_id: String::new(),
                hydrated: true,
            })
            .await;
    } else {
        let _ = bus
            .session()
            .send(SessionEvent::Started {
                session_id: String::new(),
                hydrated: false,
            })
            .await;
    }

    let initial_visible_count = runtime.llm_visible_mcp_tool_count();
    let _ = bus
        .ui_state()
        .send(UiStateEvent::SetActiveModelIdentity(
            runtime.active_model_identity(),
        ))
        .await;
    let _ = bus
        .ui_state()
        .send(UiStateEvent::SetContextWindowMaxTokens(
            runtime.max_context_tokens(),
        ))
        .await;
    for (server_name, names) in runtime.llm_visible_mcp_tool_names_by_server() {
        let _ = bus
            .ui_state()
            .send(UiStateEvent::SetMcpVisibleToolCount {
                server: server_name.clone(),
                count: names.len(),
            })
            .await;
        let _ = bus
            .ui_state()
            .send(UiStateEvent::SetMcpVisibleToolNames {
                server: server_name,
                names,
            })
            .await;
    }

    let (worker_cmd_tx, mut worker_cmd_rx) = mpsc::channel::<WorkerCommand>(256);
    let (worker_result_tx, worker_result_rx) = mpsc::channel::<TurnOutcome>(256);

    let mut worker_active = false;
    let mut active_external_prompt: Option<String> = None;
    let mut active_external_task_id: Option<String> = None;
    let mut pending_external_cancel: Option<String> = None;

    let on_agent_switch = on_agent_switch.clone();
    let worker_bus = bus.clone();
    let worker_handle = tokio::spawn(async move {
        loop {
            let command = worker_cmd_rx.recv().await;
            let Some(cmd) = command else { break };
            let should_continue = CommandRouter::dispatch(
                cmd,
                &mut runtime,
                &worker_result_tx,
                on_agent_switch.clone(),
                &worker_bus,
            )
            .await;
            if !should_continue {
                break;
            }
        }
        runtime
    });

    let (event_tx, event_rx) = mpsc::channel::<OrchestratorEvent>(256);
    let (blocking_response_tx, blocking_response_rx) = mpsc::channel::<UiRequestResponse>(32);
    let (concurrent_response_tx, concurrent_response_rx) = mpsc::channel::<UiRequestResponse>(32);

    // Subscribe to the bus channels the orchestrator consumes directly. The
    // orchestrator `select!`s over these alongside the TUI event channel, so no
    // pump tasks are needed to forward them. Subscribing before the render loop
    // spawn ensures no external/compaction event published during startup is
    // missed.
    let mut external_rx = bus.external().subscribe();
    let mut compaction_rx = bus.compaction().subscribe();

    if let Some(spawn) = spawn_render_loop {
        spawn(event_tx.clone());
    }

    // Stages
    let mut slash = SlashStage;
    let mut permission = PermissionStage;
    let mut session = SessionStage;
    let mut ui_request = UiRequestStage::new(initial_visible_count);

    // Context
    let mut ctx = OrchestrationContext {
        worker_tx: &worker_cmd_tx,
        blocking_response_tx: &blocking_response_tx,
        concurrent_response_tx: &concurrent_response_tx,
        pending: &interactive_pending,
        worker_active: &mut worker_active,
        span,
        active_external_prompt: &mut active_external_prompt,
        active_external_task_id: &mut active_external_task_id,
        pending_external_cancel: &mut pending_external_cancel,
        bus: &bus,
    };

    let stages = Stages {
        slash: &mut slash,
        permission: &mut permission,
        ui_request: &mut ui_request,
        session: &mut session,
    };

    let result = run_orchestrator_loop(
        event_rx,
        SourceChannels {
            worker_result_rx,
            blocking_response_rx,
            concurrent_response_rx,
            external_rx: &mut external_rx,
            compaction_rx: &mut compaction_rx,
            task_cancel_rx,
            a2a_task_rx,
            a2a_completion_rx,
        },
        stages,
        &mut ctx,
    )
    .await;

    let _ = worker_cmd_tx.send(WorkerCommand::Shutdown).await;
    let runtime = worker_handle
        .await
        .unwrap_or_else(|_| panic!("worker panicked"));

    let result = result.map(|()| Value::nothing(span));
    (runtime, result)
}

/// Checks an external channel for pre-formatted prompt strings (e.g., injected
/// A2A tasks) before each iteration. These prompts are dispatched with higher
/// priority than regular user input.
pub async fn run_interactive_loop_with_external_prompts<R, F>(
    runtime: R,
    config: InteractiveLoopConfig<F>,
) -> Result<Value, LabeledError>
where
    R: CoreRuntime
        + McpManagement
        + ModelSwitching
        + SessionState
        + SessionPersistence
        + Send
        + 'static,
    F: FnOnce(mpsc::Sender<OrchestratorEvent>) + Send + 'static,
{
    let (_runtime, result) = run_interactive_loop_impl(runtime, config).await;
    result
}

/// Checks an external channel for pre-formatted prompt strings (e.g., injected
/// A2A tasks) before each iteration.
pub async fn run_hydrated_interactive_loop_with_external_prompts<R, F>(
    runtime: R,
    config: InteractiveLoopConfig<F>,
) -> Result<Value, LabeledError>
where
    R: CoreRuntime
        + McpManagement
        + ModelSwitching
        + SessionState
        + SessionPersistence
        + Send
        + 'static,
    F: FnOnce(mpsc::Sender<OrchestratorEvent>) + Send + 'static,
{
    let (_runtime, result) = run_interactive_loop_impl(runtime, config).await;
    result
}

pub async fn run_single_turn<R>(
    runtime: &mut R,
    bus: &Bus,
    prompt: String,
    context: Option<String>,
    span: nu_protocol::Span,
) -> Result<Value, LabeledError>
where
    R: CoreRuntime,
{
    runtime.execute_turn(bus, prompt, context, span).await
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

/// The source channels the orchestrator loop selects over, in addition to the
/// TUI `event_rx`. Grouped so `run_orchestrator_loop` stays under clippy's
/// `too_many_arguments` threshold.
pub(crate) struct SourceChannels<'a> {
    /// Worker turn outcomes.
    pub worker_result_rx: mpsc::Receiver<TurnOutcome>,
    /// Blocking UI request responses.
    pub blocking_response_rx: mpsc::Receiver<UiRequestResponse>,
    /// Concurrent UI request responses.
    pub concurrent_response_rx: mpsc::Receiver<UiRequestResponse>,
    /// External (A2A) events from the bus.
    pub external_rx: &'a mut ExternalRx,
    /// Compaction lifecycle events from the bus.
    pub compaction_rx: &'a mut CompactionRx,
    /// A2A task cancellation IDs.
    pub task_cancel_rx: Option<mpsc::UnboundedReceiver<String>>,
    /// Incoming A2A tasks.
    pub a2a_task_rx: Option<mpsc::Receiver<IncomingTask>>,
    /// A2A completion events.
    pub a2a_completion_rx: Option<mpsc::Receiver<A2aCompletionEvent>>,
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
    let mut pending_compaction: Option<String> = None;
    let mut compaction_active = false;
    let mut quit_pending = false;

    // The task-cancel source is optional. When absent, select on a dummy
    // receiver that is never closed, so the `select!` arm always exists and the
    // loop does not spin.
    let (dummy_cancel_tx, dummy_cancel_rx) = mpsc::unbounded_channel::<String>();
    let mut task_cancel_rx = sources.task_cancel_rx.unwrap_or(dummy_cancel_rx);
    let _ = dummy_cancel_tx;

    // The incoming A2A task and completion-event sources are optional. Each is
    // polled through a helper that awaits `pending()` forever when the channel
    // is absent, so the `select!` arms always exist and the loop never spins.
    // When a channel closes, the corresponding `Option` is set to `None` so the
    // arm stops polling it.
    let mut a2a_task_rx_opt = sources.a2a_task_rx;
    let mut a2a_completion_rx_opt = sources.a2a_completion_rx;

    loop {
        tokio::select! {
            biased;

            recv = sources.external_rx.recv() => {
                match recv {
                    Ok(ExternalEvent::PromptReceived { prompt, task_id }) => {
                        handle_external_prompt(prompt, task_id, ctx).await;
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
                        if !ui_request.has_blocking_pending() {
                            slash.handle(text, ctx).await;
                        }
                    }
                    OrchestratorEvent::PermissionDecision { decision } => {
                        permission.handle(decision, ctx);
                    }
                    OrchestratorEvent::UiRequest(req) => {
                        ui_request.handle_incoming(req, ctx).await;
                    }
                    OrchestratorEvent::CancelRequested => {
                        let _ = ctx.bus.cancel().request_cancel().await;
                    }
                    OrchestratorEvent::Quit => {
                        if *ctx.worker_active {
                            quit_pending = true;
                            let _ = ctx.bus.cancel().request_cancel().await;
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
            }

            resp = sources.blocking_response_rx.recv() => {
                let Some(resp) = resp else { break };
                ui_request.handle_blocking_response(resp, ctx).await;
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
                    Ok(CompactionEvent::Requested { source }) => {
                        dispatch_compaction(source, ctx, &mut pending_compaction, &mut compaction_active).await;
                    }
                    Ok(CompactionEvent::Completed { .. } | CompactionEvent::Failed { .. }) => {
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
                let Some(incoming) = incoming else { break };
                // Format the same prompt the removed std-bridge forwarder used.
                let text: String = incoming
                    .message
                    .parts
                    .iter()
                    .filter_map(|p| match p {
                        Part::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                let prompt = format!(
                    "[A2A Task {} from {}]: {}\n\nProcess this request and respond with your answer. Your response will be automatically delivered as the task result.",
                    incoming.task_id, incoming.sender_url, text
                );
                handle_external_prompt(prompt, incoming.task_id, ctx).await;
            }

            event = recv_or_pending(&mut a2a_completion_rx_opt) => {
                let Some(event) = event else { break };
                // Format the same prompt the removed std-bridge forwarder used.
                let prompt = format!(
                    "[A2A Task {} completed by {}]: {}\n\nStatus: {}.",
                    event.task_id, event.agent_name, event.result, event.status
                );
                handle_external_prompt(prompt, event.task_id, ctx).await;
            }
        }
    }
    Ok(())
}
