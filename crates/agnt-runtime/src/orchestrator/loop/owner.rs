//! `Loop`: the owner of the interactive loop, its named phases, and the one
//! public entry.

use nu_protocol::{LabeledError, Span, Value};
use tokio::sync::mpsc;

use crate::orchestrator::config::LoopParts;
use crate::orchestrator::events::{
    OrchestratorEvent, UiRequestResponse, UiStateEvent, WorkerCommand,
};
use crate::orchestrator::loop_impl::{WORKER_CMD_CAPACITY, run_orchestrator_loop};
use crate::orchestrator::router::{CommandRouter, DispatchSources};
use crate::orchestrator::stages::OrchestrationContext;
use crate::orchestrator::turn_outcome::TurnResult;
use crate::orchestrator::{AgentSwitchHook, LoopConfig, NoAgentSwitchHook, ProductionStages};
use crate::protocol::McpManagement;
use crate::protocol::contracts::CoreRuntime;
use crate::protocol::model_switching::ModelSwitching;
use crate::session::management::{SessionPersistence, SessionState};
use agnt_kernel::bus::SessionEvent;

use super::sources::{LoopContext, LoopSources, SourceChannels};

/// Capacity of the worker result channel.
const WORKER_RESULT_CAPACITY: usize = 256;

/// Capacity of the stage response channels.
const RESPONSE_CAPACITY: usize = 32;

/// Capacity of the loop's own event channel.
const EVENT_CAPACITY: usize = 256;

/// The interactive loop's owner.
///
/// It holds the runtime, the config's seams, the channels it owns for one run,
/// and the state its phases mutate. `compose` builds it; `run` drives the named
/// phases in order.
pub struct Loop<R, F = fn(mpsc::Sender<OrchestratorEvent>), H = NoAgentSwitchHook> {
    runtime: RuntimeSlot<R>,
    config: LoopParts<F, H>,
    sources: LoopSources,
    ctx: LoopContext,
    span: Span,
}

/// Where the runtime lives while the loop runs.
enum RuntimeSlot<R> {
    /// The loop still owns the runtime.
    Ready(R),
    /// `spawn_worker` moved the runtime into the worker task. The handle yields
    /// it back; the loop keeps the command sender and the outcome receiver.
    Running {
        worker_tx: mpsc::Sender<WorkerCommand>,
        result_rx: mpsc::Receiver<TurnResult>,
        handle: tokio::task::JoinHandle<R>,
    },
}

// region:    --- Constructors

impl<R, F, H> Loop<R, F, H> {
    /// Compose a loop from its runtime and its configuration seams.
    pub fn compose(runtime: R, config: LoopConfig<F, H>) -> Self {
        let (parts, intake) = config.into_parts();
        let (blocking_response_tx, blocking_response_rx) =
            mpsc::channel::<UiRequestResponse>(RESPONSE_CAPACITY);
        let (concurrent_response_tx, concurrent_response_rx) =
            mpsc::channel::<UiRequestResponse>(RESPONSE_CAPACITY);
        let sources = LoopSources {
            blocking_response_rx,
            concurrent_response_rx,
            external_rx: parts.bus.external().subscribe(),
            compaction_rx: parts.bus.compaction().subscribe(),
            task_cancel_rx: intake.task_cancel_rx,
            a2a_task_rx: intake.a2a_task_rx,
            a2a_completion_rx: intake.a2a_completion_rx,
        };
        Self {
            runtime: RuntimeSlot::Ready(runtime),
            config: parts,
            sources,
            ctx: LoopContext::new(blocking_response_tx, concurrent_response_tx),
            span: Span::unknown(),
        }
    }
}

// endregion: --- Constructors

// region:    --- Phases

impl<R, F, H> Loop<R, F, H>
where
    R: CoreRuntime
        + McpManagement
        + ModelSwitching
        + SessionState
        + SessionPersistence
        + Send
        + 'static,
    F: FnOnce(mpsc::Sender<OrchestratorEvent>) + Send + 'static,
    H: AgentSwitchHook + Clone + Send + 'static,
{
    /// Drive the loop to completion and hand the runtime back.
    pub async fn run(mut self) -> (R, Result<Value, LabeledError>) {
        self.hydrate().await;
        self.announce_facts().await;
        self = self.spawn_worker();
        let outcome = self.select().await;
        self.teardown(outcome).await
    }

    /// Phase 1: replay the prior session to the surfaces and seed the runtime.
    ///
    /// Runs while the loop still owns the runtime; after `spawn_worker` the
    /// worker task does, so this phase has nothing left to read.
    pub(crate) async fn hydrate(&mut self) {
        let RuntimeSlot::Ready(runtime) = &mut self.runtime else {
            return;
        };
        if let Some(hydration) = self.config.hydration.take() {
            let _ = self
                .config
                .bus
                .ui_state()
                .send(UiStateEvent::HydrateTranscript {
                    messages: hydration.messages,
                    last_total_tokens: hydration.last_total_tokens,
                })
                .await;
            runtime.seed_last_total_tokens(hydration.last_total_tokens);
            let _ = self
                .config
                .bus
                .session()
                .send(SessionEvent::Started {
                    session_id: runtime.current_session_id().unwrap_or_default().to_string(),
                    hydrated: true,
                })
                .await;
        } else {
            let _ = self
                .config
                .bus
                .session()
                .send(SessionEvent::Started {
                    session_id: runtime.current_session_id().unwrap_or_default().to_string(),
                    hydrated: false,
                })
                .await;
        }
    }

    /// Phase 2: publish the facts the surfaces render before the first turn.
    pub(crate) async fn announce_facts(&mut self) {
        let RuntimeSlot::Ready(runtime) = &mut self.runtime else {
            return;
        };
        let initial_visible_count = runtime.llm_visible_mcp_tool_count();
        let _ = self
            .config
            .bus
            .ui_state()
            .send(UiStateEvent::SetActiveModelIdentity(
                runtime.active_model_identity(),
            ))
            .await;
        let _ = self
            .config
            .bus
            .ui_state()
            .send(UiStateEvent::SetContextWindowMaxTokens(
                runtime.max_context_tokens(),
            ))
            .await;
        for (server_name, names) in runtime.llm_visible_mcp_tool_names_by_server() {
            let _ = self
                .config
                .bus
                .ui_state()
                .send(UiStateEvent::SetMcpVisibleToolCount {
                    server: server_name.clone(),
                    count: names.len(),
                })
                .await;
            let _ = self
                .config
                .bus
                .ui_state()
                .send(UiStateEvent::SetMcpVisibleToolNames {
                    server: server_name,
                    names,
                })
                .await;
        }
        self.ctx.initial_visible_count = initial_visible_count;
    }

    /// Phase 3: hand the runtime to the worker task that routes its commands.
    pub(crate) fn spawn_worker(mut self) -> Self {
        let runtime = self.runtime;
        self.runtime = match runtime {
            RuntimeSlot::Ready(mut runtime) => {
                let (worker_tx, mut worker_rx) =
                    mpsc::channel::<WorkerCommand>(WORKER_CMD_CAPACITY);
                let (result_tx, result_rx) = mpsc::channel::<TurnResult>(WORKER_RESULT_CAPACITY);
                let on_agent_switch = self.config.on_agent_switch.clone();
                let worker_sources = DispatchSources {
                    turn: self.config.bus.turn().clone(),
                    compaction: self.config.bus.compaction().clone(),
                    session: self.config.bus.session().clone(),
                    ui_state: self.config.bus.ui_state().clone(),
                };
                let handle = tokio::spawn(async move {
                    while let Some(cmd) = worker_rx.recv().await {
                        let should_continue = CommandRouter::dispatch(
                            cmd,
                            &mut runtime,
                            &result_tx,
                            on_agent_switch.clone(),
                            &worker_sources,
                        )
                        .await;
                        if !should_continue {
                            break;
                        }
                    }
                    runtime
                });
                RuntimeSlot::Running {
                    worker_tx,
                    result_rx,
                    handle,
                }
            }
            running => running,
        };
        self
    }

    /// Phase 4: select over the sources and dispatch each event to its stage.
    pub(crate) async fn select(&mut self) -> Result<(), LabeledError> {
        let RuntimeSlot::Running {
            worker_tx,
            result_rx,
            ..
        } = &mut self.runtime
        else {
            return Err(LabeledError::new("the orchestrator loop has no worker"));
        };

        let (event_tx, event_rx) = mpsc::channel::<OrchestratorEvent>(EVENT_CAPACITY);
        if let Some(sink) = self.config.event_sink.take() {
            sink(event_tx.clone());
        }
        let _event_tx = event_tx;

        let mut production = ProductionStages::new(self.ctx.initial_visible_count);

        let mut ctx = OrchestrationContext {
            worker_tx,
            blocking_response_tx: &self.ctx.blocking_response_tx,
            concurrent_response_tx: &self.ctx.concurrent_response_tx,
            pending: &self.config.interactive_pending,
            worker_active: &mut self.ctx.worker_active,
            span: self.span,
            active_external_prompt: &mut self.ctx.active_external_prompt,
            active_external_task_id: &mut self.ctx.active_external_task_id,
            pending_external_cancel: &mut self.ctx.pending_external_cancel,
            pending_a2a_task_id: &mut self.ctx.pending_a2a_task_id,
            turn: self.config.bus.turn(),
            cancel: self.config.bus.cancel(),
            session: self.config.bus.session(),
            compaction: self.config.bus.compaction(),
            ui_event: self.config.bus.ui_event(),
            ui_state: self.config.bus.ui_state(),
            task_store: self.config.task_store.as_ref(),
        };

        let sources = SourceChannels {
            worker_result_rx: result_rx,
            blocking_response_rx: &mut self.sources.blocking_response_rx,
            concurrent_response_rx: &mut self.sources.concurrent_response_rx,
            external_rx: &mut self.sources.external_rx,
            compaction_rx: &mut self.sources.compaction_rx,
            task_cancel_rx: self.sources.task_cancel_rx.take(),
            a2a_task_rx: self.sources.a2a_task_rx.take(),
            a2a_completion_rx: self.sources.a2a_completion_rx.take(),
        };

        run_orchestrator_loop(event_rx, sources, production.stages(), &mut ctx).await
    }

    /// Phase 5: stop the worker, take the runtime back, and close the session.
    ///
    /// One teardown site covers every loop exit (quit, channel close, error).
    /// The session's cumulative usage rides the event: the runtime owns the
    /// cells, so the totals cover this session only.
    pub(crate) async fn teardown(
        self,
        outcome: Result<(), LabeledError>,
    ) -> (R, Result<Value, LabeledError>) {
        let runtime = match self.runtime {
            RuntimeSlot::Running {
                worker_tx, handle, ..
            } => {
                let _ = worker_tx.send(WorkerCommand::Shutdown).await;
                match handle.await {
                    Ok(runtime) => runtime,
                    Err(_) => panic!("worker panicked"),
                }
            }
            RuntimeSlot::Ready(runtime) => {
                return (
                    runtime,
                    Err(LabeledError::new("the orchestrator loop never ran")),
                );
            }
        };

        let usage = runtime.session_usage();
        let _ = self
            .config
            .bus
            .session()
            .send(SessionEvent::Ended {
                session_id: runtime.current_session_id().unwrap_or_default().to_string(),
                input_tokens: usage.input_tokens,
                output_tokens: usage.output_tokens,
                total_tokens: usage.total_tokens,
                cost: usage.cost,
                cost_complete: usage.cost_complete,
            })
            .await;

        let result = outcome.map(|()| Value::nothing(self.span));
        (runtime, result)
    }
}

// endregion: --- Phases

// region:    --- Entry

/// The one public loop entry: compose a [`Loop`] and drive it to completion.
pub async fn run<R, F, H>(runtime: R, config: LoopConfig<F, H>) -> Result<Value, LabeledError>
where
    R: CoreRuntime
        + McpManagement
        + ModelSwitching
        + SessionState
        + SessionPersistence
        + Send
        + 'static,
    F: FnOnce(mpsc::Sender<OrchestratorEvent>) + Send + 'static,
    H: AgentSwitchHook + Clone + Send + 'static,
{
    let (_runtime, result) = Loop::compose(runtime, config).run().await;
    result
}

// endregion: --- Entry
