//! `Loop`: the owner of the interactive loop, its named phases, and the one
//! public entry.

use nu_protocol::{LabeledError, Span, Value};
use tokio::sync::mpsc;

use crate::catalog::PickerCatalogSource;
use crate::orchestrator::config::LoopParts;
use crate::orchestrator::events::{OrchestratorEvent, UiRequestResponse, WorkerCommand};
use crate::orchestrator::external::{ExternalTaskStore, NoExternalTaskStore};
use crate::orchestrator::loop_impl::{WORKER_CMD_CAPACITY, run_orchestrator_loop};
use crate::orchestrator::router::{CommandRouter, DispatchSources};
use crate::orchestrator::stages::OrchestrationContext;
use crate::orchestrator::turn_outcome::TurnResult;
use crate::orchestrator::{AgentSwitchHook, LoopConfig, NoAgentSwitchHook, ProductionStages};
use crate::protocol::McpManagement;
use crate::protocol::contracts::CoreRuntime;
use crate::protocol::model_switching::ModelSwitching;
use crate::session::management::{SessionPersistence, SessionState};
use agnt_kernel::bus::{FactRecord, LifecycleEvent};
use agnt_kernel::{SessionId, SessionLifecycle, SessionRecord};

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
pub struct Loop<
    R,
    F = fn(mpsc::Sender<OrchestratorEvent>),
    H = NoAgentSwitchHook,
    S = NoExternalTaskStore,
> {
    runtime: RuntimeSlot<R>,
    config: LoopParts<F, H, S>,
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

impl<R, F, H, S> Loop<R, F, H, S> {
    /// Compose a loop from its runtime and its configuration seams.
    pub fn compose(runtime: R, config: LoopConfig<F, H, S>) -> Self {
        let (parts, intake) = config.into_parts();
        let (blocking_response_tx, blocking_response_rx) =
            mpsc::channel::<UiRequestResponse>(RESPONSE_CAPACITY);
        let (concurrent_response_tx, concurrent_response_rx) =
            mpsc::channel::<UiRequestResponse>(RESPONSE_CAPACITY);
        let sources = LoopSources {
            blocking_response_rx,
            concurrent_response_rx,
            lifecycle_rx: parts.bus.lifecycle().subscribe(),
            control_rx: parts.bus.control().subscribe(),
            task_cancel_rx: intake.task_cancel_rx,
            external_task_rx: intake.external_task_rx,
            external_completion_rx: intake.external_completion_rx,
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

impl<R, F, H, S> Loop<R, F, H, S>
where
    R: CoreRuntime
        + McpManagement
        + ModelSwitching
        + SessionState
        + SessionPersistence
        + PickerCatalogSource
        + Send
        + 'static,
    F: FnOnce(mpsc::Sender<OrchestratorEvent>) + Send + 'static,
    H: AgentSwitchHook + Clone + Send + 'static,
    S: ExternalTaskStore,
{
    /// Drive the loop to completion and hand the runtime back.
    ///
    /// The runtime comes back as `None` when the worker task failed: its frame
    /// held the runtime, so a failed join leaves nothing to hand back.
    pub async fn run(mut self) -> (Option<R>, Result<Value, LabeledError>) {
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
            // The construction-time seed: the surface folds the projected
            // records before the loop publishes anything else, so a resumed
            // session renders exactly what a live one would.
            let (records, _projected_total) =
                crate::protocol::contracts::project_snapshots(hydration.messages);
            let _ = self.config.bus.stream().publish(SessionRecord::Session(
                SessionLifecycle::Started {
                    session_id: SessionId::new(
                        runtime.current_session_id().unwrap_or_default().to_string(),
                    ),
                    hydrated: true,
                },
            ));
            for record in records {
                let _ = self.config.bus.stream().publish(record);
            }
            let _ = self
                .config
                .bus
                .lifecycle()
                .publish(LifecycleEvent::SessionStarted {
                    session_id: runtime.current_session_id().unwrap_or_default().to_string(),
                    hydrated: true,
                });
        } else {
            let _ = self.config.bus.stream().publish(SessionRecord::Session(
                SessionLifecycle::Started {
                    session_id: SessionId::new(
                        runtime.current_session_id().unwrap_or_default().to_string(),
                    ),
                    hydrated: false,
                },
            ));
            let _ = self
                .config
                .bus
                .lifecycle()
                .publish(LifecycleEvent::SessionStarted {
                    session_id: runtime.current_session_id().unwrap_or_default().to_string(),
                    hydrated: false,
                });
        }
    }

    /// Phase 2: publish the facts the surfaces render before the first turn.
    ///
    /// Facts carry no retention: a surface that has not subscribed yet does not
    /// observe these. The initial state is therefore seeded directly on the
    /// surface at construction; this phase announces the same values on the
    /// standard channel for every live subscriber.
    pub(crate) async fn announce_facts(&mut self) {
        let RuntimeSlot::Ready(runtime) = &mut self.runtime else {
            return;
        };
        let initial_visible_count = runtime.llm_visible_mcp_tool_count();
        let _ = self
            .config
            .bus
            .facts()
            .publish(FactRecord::ActiveModelIdentity(
                runtime.active_model_identity(),
            ));
        let _ = self
            .config
            .bus
            .facts()
            .publish(FactRecord::ContextWindowMaxTokens(
                runtime.max_context_tokens(),
            ));
        for (server_name, names) in runtime.llm_visible_mcp_tool_names_by_server() {
            let _ = self
                .config
                .bus
                .facts()
                .publish(FactRecord::McpVisibleToolCount {
                    server: server_name.clone(),
                    count: names.len(),
                });
            let _ = self
                .config
                .bus
                .facts()
                .publish(FactRecord::McpVisibleToolNames {
                    server: server_name,
                    names,
                });
        }
        self.ctx.initial_visible_count = initial_visible_count;
    }

    /// Phase 3: hand the runtime to the worker task that routes its commands.
    pub(crate) fn spawn_worker(mut self) -> Self {
        let runtime = self.runtime;
        self.runtime = match runtime {
            RuntimeSlot::Ready(mut runtime) => {
                // The picker reader is built while the loop still owns the
                // runtime: the worker takes it into its own task, so the
                // catalogs a surface asks for mid-turn come from here.
                self.ctx.catalog = runtime.picker_catalog();
                let (worker_tx, mut worker_rx) =
                    mpsc::channel::<WorkerCommand>(WORKER_CMD_CAPACITY);
                let (result_tx, result_rx) = mpsc::channel::<TurnResult>(WORKER_RESULT_CAPACITY);
                let on_agent_switch = self.config.on_agent_switch.clone();
                let worker_sources = DispatchSources {
                    control: self.config.bus.control().clone(),
                    lifecycle: self.config.bus.lifecycle().clone(),
                    stream: self.config.bus.stream().clone(),
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
            catalog: &mut self.ctx.catalog,
            span: self.span,
            stream: self.config.bus.stream(),
            control: self.config.bus.control(),
            cancel: self.config.bus.cancel(),
            lifecycle: self.config.bus.lifecycle(),
            narration: self.config.bus.narration(),
            facts: self.config.bus.facts(),
        };

        let sources = SourceChannels {
            worker_result_rx: result_rx,
            blocking_response_rx: &mut self.sources.blocking_response_rx,
            concurrent_response_rx: &mut self.sources.concurrent_response_rx,
            lifecycle_rx: &mut self.sources.lifecycle_rx,
            control_rx: &mut self.sources.control_rx,
            task_cancel_rx: self.sources.task_cancel_rx.take(),
            external_task_rx: self.sources.external_task_rx.take(),
            external_completion_rx: self.sources.external_completion_rx.take(),
            external_task_store: self.config.external_task_store.as_ref(),
        };

        run_orchestrator_loop(event_rx, sources, production.stages(), &mut ctx).await
    }

    /// Phase 5: stop the worker, take the runtime back, and close the session.
    ///
    /// One teardown site covers every loop exit (quit, channel close, error).
    /// The session's cumulative usage rides the event: the runtime owns the
    /// cells, so the totals cover this session only.
    ///
    /// A worker that fails while joined hands no runtime back: the task's frame
    /// is dropped with it, so the usage the `SessionEnded` event needs is gone.
    /// Teardown then returns the join failure as the outcome and no runtime,
    /// instead of panicking mid-close.
    pub(crate) async fn teardown(
        self,
        outcome: Result<(), LabeledError>,
    ) -> (Option<R>, Result<Value, LabeledError>) {
        let span = self.span;
        let (runtime, result) = match self.runtime {
            RuntimeSlot::Running {
                worker_tx, handle, ..
            } => {
                let _ = worker_tx.send(WorkerCommand::Shutdown).await;
                match handle.await {
                    Ok(runtime) => (Some(runtime), outcome.map(|()| Value::nothing(span))),
                    Err(join_err) => {
                        let message = worker_failure_message(join_err);
                        log::error!("{message}");
                        (None, Err(LabeledError::new(message)))
                    }
                }
            }
            RuntimeSlot::Ready(runtime) => (
                Some(runtime),
                Err(LabeledError::new("the orchestrator loop never ran")),
            ),
        };

        let Some(runtime) = runtime else {
            return (None, result);
        };

        let usage = runtime.session_usage();
        let _ = self
            .config
            .bus
            .lifecycle()
            .publish(LifecycleEvent::SessionEnded {
                session_id: runtime.current_session_id().unwrap_or_default().to_string(),
                input_tokens: usage.input_tokens,
                output_tokens: usage.output_tokens,
                total_tokens: usage.total_tokens,
                cost: usage.cost,
                cost_complete: usage.cost_complete,
            });

        (Some(runtime), result)
    }
}

// endregion: --- Phases

// region:    --- Support

/// The message a failed worker join carries into the loop's outcome.
fn worker_failure_message(join_err: tokio::task::JoinError) -> String {
    if !join_err.is_panic() {
        return format!("the orchestrator worker task failed: {join_err}");
    }
    match join_err.try_into_panic() {
        Ok(payload) => format!(
            "the orchestrator worker panicked: {}",
            panic_payload_text(payload.as_ref())
        ),
        Err(join_err) => format!("the orchestrator worker task failed: {join_err}"),
    }
}

/// A panic payload's text, for the message that names the failure.
fn panic_payload_text(payload: &(dyn std::any::Any + Send)) -> &str {
    if let Some(text) = payload.downcast_ref::<&'static str>() {
        return text;
    }
    if let Some(text) = payload.downcast_ref::<String>() {
        return text;
    }
    "a non-string panic payload"
}

// endregion: --- Support

// region:    --- Entry

/// The one public loop entry: compose a [`Loop`] and drive it to completion.
pub async fn run<R, F, H, S>(runtime: R, config: LoopConfig<F, H, S>) -> Result<Value, LabeledError>
where
    R: CoreRuntime
        + McpManagement
        + ModelSwitching
        + SessionState
        + SessionPersistence
        + PickerCatalogSource
        + Send
        + 'static,
    F: FnOnce(mpsc::Sender<OrchestratorEvent>) + Send + 'static,
    H: AgentSwitchHook + Clone + Send + 'static,
    S: ExternalTaskStore,
{
    let (_runtime, result) = Loop::compose(runtime, config).run().await;
    result
}

// endregion: --- Entry
