pub mod bridge;
pub mod router;
pub mod stages;
pub mod turn_outcome;

#[cfg(test)]
#[path = "stage_test.rs"]
mod stage_test;
#[cfg(test)]
mod test;
#[cfg(test)]
mod test_shared;
#[cfg(test)]
mod turn_outcome_test;

use std::sync::{Arc, Mutex, mpsc as std_mpsc};
use tokio::sync::{broadcast, mpsc};

use nu_protocol::{LabeledError, Span, Value};

use crate::bus::{Bus, CancelEvent, ExternalEvent, SessionEvent, TurnEvent};
use crate::conversation::runtime::PendingPermissions;

use crate::orchestrator::{
    router::CommandRouter,
    stages::{
        CompactionHandler, OrchestrationContext, PermissionHandler, SessionHandler, SlashHandler,
        UiRequestHandler, compaction::CompactionStage, permission::PermissionStage,
        session::SessionStage, slash::SlashStage, ui_request::UiRequestStage,
    },
    turn_outcome::TurnOutcome,
};
use crate::protocol::{
    compaction::CompactionTriggerSource,
    compaction_runtime::Compaction,
    contracts::{CoreRuntime, McpUsabilityState, ProgressUi, SharedUiAction, UiMessageSnapshot},
    event::{PermissionDecisionSubmission, UiEvent},
    mcp_management::McpManagement,
    model_switching::ModelSwitching,
    session_management::{SessionPersistence, SessionState},
};
use crate::session::SessionInfo;

/// Configuration for the interactive loop.
///
/// Groups common arguments that would otherwise be passed individually,
/// keeping function signatures under clippy's `too_many_arguments` threshold.
pub struct InteractiveLoopConfig<F = fn(mpsc::Sender<OrchestratorEvent>)> {
    /// The span to use for values created during the loop.
    pub span: Span,
    /// Pending permission requests awaiting user decisions.
    pub interactive_pending: Option<PendingPermissions>,
    /// Optional channel for receiving task IDs to cancel (e.g., A2A tasks).
    pub task_cancel_rx: Option<std_mpsc::Receiver<String>>,
    /// Shared cancellation bus.
    pub bus: Bus,
    /// Optional hydration config for resuming a prior session.
    pub hydration: Option<HydrationConfig>,
    /// Optional callback invoked after a successful agent switch.
    /// Receives the new agent's identity (name) and optional description.
    /// Used by the binary layer to update the A2A agent card.
    pub on_agent_switch: Option<OnAgentSwitch>,
    /// Optional closure that spawns the TUI render loop.
    /// Receives the orchestrator event sender.
    pub spawn_render_loop: Option<F>,
}

impl InteractiveLoopConfig<fn(mpsc::Sender<OrchestratorEvent>)> {
    /// Create a new config with the given span and all other fields set to `None`.
    pub fn new(span: Span) -> Self {
        Self {
            span,
            interactive_pending: None,
            task_cancel_rx: None,
            bus: crate::bus::create_bus(),
            hydration: None,
            on_agent_switch: None,
            spawn_render_loop: None,
        }
    }
}

impl<F: FnOnce(mpsc::Sender<OrchestratorEvent>) + Send + 'static> InteractiveLoopConfig<F> {
    /// Set the interactive pending permissions.
    pub fn with_interactive_pending(mut self, pending: Option<PendingPermissions>) -> Self {
        self.interactive_pending = pending;
        self
    }

    /// Set the task cancel receiver.
    pub fn with_task_cancel_rx(mut self, rx: Option<std_mpsc::Receiver<String>>) -> Self {
        self.task_cancel_rx = rx;
        self
    }

    /// Set the shared cancellation bus.
    pub fn with_bus(mut self, bus: Bus) -> Self {
        self.bus = bus;
        self
    }

    /// Set the hydration config using a builder pattern.
    pub fn with_hydration(
        mut self,
        messages: Vec<UiMessageSnapshot>,
        last_total_tokens: Option<u64>,
    ) -> Self {
        self.hydration = Some(HydrationConfig {
            messages,
            last_total_tokens,
        });
        self
    }

    /// Set the on-agent-switch callback.
    ///
    /// The callback receives the new agent's identity (name) and optional
    /// description after a successful switch. Used by the binary layer to
    /// update the A2A agent card.
    pub fn with_on_agent_switch(mut self, callback: OnAgentSwitch) -> Self {
        self.on_agent_switch = Some(callback);
        self
    }

    /// Set the render-loop spawner closure.
    pub fn with_spawn_render_loop<F2: FnOnce(mpsc::Sender<OrchestratorEvent>) + Send + 'static>(
        self,
        f: F2,
    ) -> InteractiveLoopConfig<F2> {
        let InteractiveLoopConfig {
            span,
            interactive_pending,
            task_cancel_rx,
            bus,
            hydration,
            on_agent_switch,
            spawn_render_loop: _,
        } = self;
        InteractiveLoopConfig {
            span,
            interactive_pending,
            task_cancel_rx,
            bus,
            hydration,
            on_agent_switch,
            spawn_render_loop: Some(f),
        }
    }
}

/// Configuration for hydrating a prior session into the interactive loop.
pub struct HydrationConfig {
    /// Messages from the prior session to display in the transcript.
    pub messages: Vec<UiMessageSnapshot>,
    /// The total token count from the prior session, if known.
    pub last_total_tokens: Option<u64>,
}

pub type McpToggleResult = (
    Result<McpUsabilityState, String>,
    usize,
    usize,
    Vec<(String, Vec<String>)>,
);
pub type ModelSwitchResult = Result<(String, Option<u64>), String>;
pub type AgentSwitchResult = Result<(String, String, Option<u64>, Option<String>), String>;
pub type SessionSwitchResult = Result<Vec<UiMessageSnapshot>, String>;
pub type RefreshSessionPickerResult = Result<Vec<SessionInfo>, String>;
pub(crate) type PendingCompactionTrigger = mpsc::Receiver<Option<String>>;

/// Events flowing into the orchestrator loop from all components (TUI, worker,
/// bus pump). The orchestrator drains this single channel with `while let Some`.
pub enum OrchestratorEvent {
    // ── From TUI (user actions) ──
    PromptSubmitted {
        text: String,
    },
    PermissionDecision {
        decision: PermissionDecisionSubmission,
    },
    UiRequest(UiRequest),

    // ── From worker (async results) ──
    WorkerResult(TurnOutcome),
    BlockingResponse(UiRequestResponse),
    ConcurrentResponse(UiRequestResponse),
    CompactionResult {
        message: Option<String>,
    },

    // ── Signals ──
    ExternalPrompt {
        prompt: String,
        task_id: String,
    },
    ExternalCancel {
        task_id: String,
    },
    CancelRequested,
    Quit,
    FatalError(LabeledError),
}

/// A request from the TUI that requires an async response from the worker.
#[derive(Clone)]
pub enum UiRequest {
    SwitchModel { spec: String },
    SwitchAgent { name: String },
    SwitchSession { id: String },
    ToggleMcp { server: String, enable: bool },
    RefreshSessionPicker,
}

/// Per-type response to a `UiRequest`.
#[derive(Debug)]
pub enum UiRequestResponse {
    ModelSwitch(ModelSwitchResult),
    AgentSwitch(AgentSwitchResult),
    SessionSwitch {
        id: String,
        result: SessionSwitchResult,
    },
    McpToggle {
        server: String,
        result: Result<McpUsabilityState, String>,
        total: usize,
        server_count: usize,
        names_by_server: Vec<(String, Vec<String>)>,
    },
    SessionRefresh(RefreshSessionPickerResult),
}

/// UI state updates broadcast to the TUI render loop.
#[derive(Clone)]
pub enum UiStateEvent {
    SetActiveModelIdentity(String),
    SetActiveAgentIdentity(String),
    SetActivePersonaIcon(Option<String>),
    SetContextWindowMaxTokens(Option<u64>),
    ClearTranscript,
    HydrateTranscript {
        messages: Vec<UiMessageSnapshot>,
        last_total_tokens: Option<u64>,
    },
    SetMcpServerState {
        server: String,
        state: McpUsabilityState,
        error: Option<String>,
        total: usize,
    },
    SetMcpVisibleToolCount {
        server: String,
        count: usize,
    },
    SetMcpVisibleToolNames {
        server: String,
        names: Vec<String>,
    },
    SetSessionPickerOptions(Vec<SessionInfo>),
    DisplayIncomingMessage(String),
    ExecuteSharedUiAction(SharedUiAction),
    PushStartupLogo,
}

/// Callback invoked after a successful agent switch.
/// Receives the new agent's identity (name), optional description, and optional icon.
pub type OnAgentSwitch = Arc<dyn Fn(String, Option<String>, Option<String>) + Send + Sync>;

pub enum WorkerCommand {
    ExecuteTurn {
        prompt: String,
        span: Span,
    },
    HandleUiRequest {
        request: UiRequest,
        response_tx: mpsc::Sender<UiRequestResponse>,
    },
    EvaluateAutoCompaction {
        response_tx: mpsc::Sender<Option<String>>,
    },
    ExecuteCompactionTrigger {
        source: CompactionTriggerSource,
        response_tx: mpsc::Sender<Option<String>>,
    },
    ClearSession,
    NewSession,
    Shutdown,
}

struct WorkerProgressUi {
    events: mpsc::UnboundedSender<UiEvent>,
    cancel_rx: Arc<Mutex<broadcast::Receiver<CancelEvent>>>,
}

impl ProgressUi for WorkerProgressUi {
    fn emit(&mut self, event: &UiEvent) {
        let _ = self.events.send(event.clone());
    }

    fn flush(&mut self) {}

    fn take_cancel_requested(&self) -> bool {
        let mut rx = self.cancel_rx.lock().expect("cancel_rx mutex poisoned");
        matches!(
            rx.try_recv(),
            Ok(CancelEvent::Requested) | Err(broadcast::error::TryRecvError::Lagged(_))
        )
    }
}

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
        + Compaction
        + Send
        + 'static,
    F: FnOnce(mpsc::Sender<OrchestratorEvent>) + Send + 'static,
{
    let InteractiveLoopConfig {
        span,
        interactive_pending,
        task_cancel_rx,
        bus,
        hydration,
        ref on_agent_switch,
        spawn_render_loop,
    } = config;

    if let Some(hydration) = hydration {
        let _ = bus.ui_state().send(UiStateEvent::HydrateTranscript {
            messages: hydration.messages,
            last_total_tokens: hydration.last_total_tokens,
        });
        runtime.seed_last_total_tokens(hydration.last_total_tokens);
        let _ = bus.session().send(SessionEvent::Started {
            session_id: String::new(),
            hydrated: true,
        });
    } else {
        let _ = bus.session().send(SessionEvent::Started {
            session_id: String::new(),
            hydrated: false,
        });
    }

    let initial_visible_count = runtime.llm_visible_mcp_tool_count();
    let _ = bus.ui_state().send(UiStateEvent::SetActiveModelIdentity(
        runtime.active_model_identity(),
    ));
    let _ = bus.ui_state().send(UiStateEvent::SetContextWindowMaxTokens(
        runtime.max_context_tokens(),
    ));
    for (server_name, names) in runtime.llm_visible_mcp_tool_names_by_server() {
        let _ = bus.ui_state().send(UiStateEvent::SetMcpVisibleToolCount {
            server: server_name.clone(),
            count: names.len(),
        });
        let _ = bus.ui_state().send(UiStateEvent::SetMcpVisibleToolNames {
            server: server_name,
            names,
        });
    }

    let (worker_cmd_tx, mut worker_cmd_rx) = mpsc::channel::<WorkerCommand>(256);
    let (worker_event_tx, mut worker_event_rx) = mpsc::unbounded_channel::<UiEvent>();
    let (worker_result_tx, mut worker_result_rx) = mpsc::channel::<TurnOutcome>(256);

    let mut worker_active = false;
    let mut should_evaluate_compaction = true;
    let mut active_external_prompt: Option<String> = None;
    let mut active_external_task_id: Option<String> = None;
    let mut pending_external_cancel: Option<String> = None;

    let mut worker_ui = WorkerProgressUi {
        events: worker_event_tx,
        cancel_rx: Arc::new(Mutex::new(bus.cancel().subscribe())),
    };

    // Spawn the worker task with the owned runtime. The worker processes
    // commands (ExecuteTurn, compaction, MCP toggle, model/agent switch)
    // independently, allowing the main loop to pump the UI and run stages
    // concurrently. The runtime is returned when the worker completes.
    let on_agent_switch = on_agent_switch.clone();
    let worker_handle = tokio::spawn(async move {
        loop {
            let command = worker_cmd_rx.recv().await;
            let Some(cmd) = command else { break };
            let should_continue = CommandRouter::dispatch(
                cmd,
                &mut runtime,
                &mut worker_ui,
                &worker_result_tx,
                on_agent_switch.clone(),
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

    spawn_blocking_bridge(blocking_response_rx, event_tx.clone());
    spawn_concurrent_bridge(concurrent_response_rx, event_tx.clone());
    spawn_bus_pump(&bus, event_tx.clone());
    spawn_task_cancel_bridge(task_cancel_rx, event_tx.clone());

    if let Some(spawn) = spawn_render_loop {
        spawn(event_tx.clone());
    }

    // Worker result bridge: forward worker outcomes to the event channel.
    let worker_result_tx_clone = event_tx.clone();
    tokio::spawn(async move {
        while let Some(outcome) = worker_result_rx.recv().await {
            let _ = worker_result_tx_clone
                .send(OrchestratorEvent::WorkerResult(outcome))
                .await;
        }
    });

    // Worker event bridge: forward worker UiEvents to the corresponding bus
    // channels. Only permission events (which arrive via the worker's ui_tx,
    // not the bus) are re-published here. Lifecycle events (tool/llm/warning/
    // compaction/turn) are already published on their bus channels by the hooks
    // and executor, and the render loop subscribes to those channels directly;
    // re-publishing them would re-inject them into the same bus BusForwarder
    // drains, causing an infinite feedback loop that repeats the transcript.
    let worker_bus = bus.clone();
    tokio::spawn(async move {
        while let Some(event) = worker_event_rx.recv().await {
            match bridge::bridge_action(event) {
                bridge::BridgeAction::PublishPermission(permission_event) => {
                    let _ = worker_bus.permission().send(permission_event);
                }
                bridge::BridgeAction::Ignore => {}
            }
        }
    });

    // Stages
    let mut slash = SlashStage::new();
    let mut permission = PermissionStage::new();
    let mut session = SessionStage::new();
    let mut compaction = CompactionStage::new();
    let mut ui_request = UiRequestStage::new(initial_visible_count);

    // Context
    let mut ctx = OrchestrationContext {
        worker_tx: &worker_cmd_tx,
        blocking_response_tx: &blocking_response_tx,
        concurrent_response_tx: &concurrent_response_tx,
        pending: &interactive_pending,
        worker_active: &mut worker_active,
        should_evaluate_compaction: &mut should_evaluate_compaction,
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
        compaction: &mut compaction,
        session: &mut session,
    };

    let result = run_orchestrator_loop(event_rx, event_tx.clone(), stages, &mut ctx).await;

    let _ = worker_cmd_tx.send(WorkerCommand::Shutdown).await;
    let runtime = worker_handle
        .await
        .unwrap_or_else(|_| panic!("worker panicked"));

    let _ = bus.session().send(SessionEvent::Ended {
        session_id: String::new(),
    });

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
        + Compaction
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
        + Compaction
        + Send
        + 'static,
    F: FnOnce(mpsc::Sender<OrchestratorEvent>) + Send + 'static,
{
    let (_runtime, result) = run_interactive_loop_impl(runtime, config).await;
    result
}

pub async fn run_single_turn<R, U>(
    runtime: &mut R,
    ui: &mut U,
    prompt: String,
    context: Option<String>,
    span: Span,
) -> Result<Value, LabeledError>
where
    R: CoreRuntime,
    U: ProgressUi + Send,
{
    runtime.execute_turn(ui, prompt, context, span).await
}

/// The orchestrator event loop. Drains a single `OrchestratorEvent` channel and
/// dispatches each event to the appropriate stage handler.
///
/// Generic over the five stage traits (DIP + ISP). The loop owns no stage state;
/// stages are passed in as `&mut` and mutated in place.
pub(crate) struct Stages<'a, S, P, U, C, Se> {
    pub slash: &'a mut S,
    pub permission: &'a mut P,
    pub ui_request: &'a mut U,
    pub compaction: &'a mut C,
    pub session: &'a mut Se,
}

pub(crate) async fn run_orchestrator_loop<S, P, U, C, Se>(
    mut event_rx: mpsc::Receiver<OrchestratorEvent>,
    event_tx: mpsc::Sender<OrchestratorEvent>,
    stages: Stages<'_, S, P, U, C, Se>,
    ctx: &mut OrchestrationContext<'_>,
) -> Result<(), LabeledError>
where
    S: SlashHandler,
    P: PermissionHandler,
    U: UiRequestHandler,
    C: CompactionHandler,
    Se: SessionHandler,
{
    let Stages {
        slash,
        permission,
        ui_request,
        compaction,
        session,
    } = stages;
    let mut pending_compaction_rx: Option<mpsc::Receiver<Option<String>>> = None;
    let mut quit_pending = false;
    loop {
        let ev = tokio::select! {
            biased;
            message = async {
                if let Some(rx) = pending_compaction_rx.as_mut() {
                    rx.recv().await.unwrap_or(None)
                } else {
                    std::future::pending::<Option<String>>().await
                }
            } => OrchestratorEvent::CompactionResult { message },
            ev = event_rx.recv() => {
                let Some(ev) = ev else { break; };
                ev
            }
        };
        match ev {
            OrchestratorEvent::PromptSubmitted { text } => {
                if !ui_request.has_blocking_pending() {
                    slash.handle(text, ctx).await;
                    if let Some(rx) = slash.take_pending_compaction_trigger() {
                        pending_compaction_rx = Some(rx);
                    }
                }
            }
            OrchestratorEvent::CompactionResult { message } => {
                compaction.handle_result(message, ctx);
                *ctx.should_evaluate_compaction = false;
                pending_compaction_rx = None;
                if quit_pending && !*ctx.worker_active {
                    break;
                }
                // Re-arm compaction evaluation after a slash compaction result, mirroring
                // the auto-compaction path after WorkerResult.
                if *ctx.should_evaluate_compaction
                    && !*ctx.worker_active
                    && !compaction.has_pending_auto_compaction()
                {
                    let (response_tx, response_rx) = mpsc::channel::<Option<String>>(1);
                    let _ = ctx
                        .worker_tx
                        .send(WorkerCommand::EvaluateAutoCompaction { response_tx })
                        .await;
                    compaction.set_pending_auto_compaction();
                    spawn_compaction_bridge(response_rx, event_tx.clone());
                }
            }
            OrchestratorEvent::PermissionDecision { decision } => {
                permission.handle(decision, ctx);
            }
            OrchestratorEvent::UiRequest(req) => {
                ui_request.handle_incoming(req, ctx).await;
            }
            OrchestratorEvent::BlockingResponse(resp) => {
                ui_request.handle_blocking_response(resp, ctx);
                if quit_pending && !ui_request.has_pending() {
                    break;
                }
            }
            OrchestratorEvent::ConcurrentResponse(resp) => {
                ui_request.handle_concurrent_response(resp, ctx);
                if quit_pending && !ui_request.has_pending() {
                    break;
                }
            }
            OrchestratorEvent::WorkerResult(outcome) => {
                session.handle_outcome(outcome, ctx);
                // Worker is now idle — drain queued blocking requests.
                ui_request.drain_queued(ctx).await;
                // Re-arm compaction evaluation after turn completion.
                if *ctx.should_evaluate_compaction
                    && !*ctx.worker_active
                    && !compaction.has_pending_auto_compaction()
                {
                    let (response_tx, response_rx) = mpsc::channel::<Option<String>>(1);
                    let _ = ctx
                        .worker_tx
                        .send(WorkerCommand::EvaluateAutoCompaction { response_tx })
                        .await;
                    compaction.set_pending_auto_compaction();
                    spawn_compaction_bridge(response_rx, event_tx.clone());
                }
                // If a quit was requested while the worker was active and the
                // worker is now idle, exit the loop.
                if quit_pending && !*ctx.worker_active {
                    break;
                }
            }
            OrchestratorEvent::ExternalPrompt { prompt, task_id } => {
                if !*ctx.worker_active {
                    let _ = ctx
                        .bus
                        .ui_state()
                        .send(UiStateEvent::DisplayIncomingMessage(prompt.clone()));
                    *ctx.active_external_prompt = Some(prompt.clone());
                    *ctx.active_external_task_id = Some(task_id.clone());
                    if ctx.pending_external_cancel.as_deref() == Some(task_id.as_str()) {
                        *ctx.pending_external_cancel = None;
                        let _ = ctx.bus.cancel().send(CancelEvent::Requested);
                    }
                    let _ = ctx.bus.turn().send(TurnEvent::Started {
                        prompt: prompt.clone(),
                        task_id: Some(task_id),
                    });
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
            OrchestratorEvent::ExternalCancel { task_id } => {
                if ctx.active_external_task_id.as_deref() == Some(task_id.as_str()) {
                    let _ = ctx.bus.cancel().send(CancelEvent::Requested);
                } else {
                    *ctx.pending_external_cancel = Some(task_id);
                }
            }
            OrchestratorEvent::CancelRequested => {
                let _ = ctx.bus.cancel().send(CancelEvent::Requested);
            }
            OrchestratorEvent::Quit => {
                if *ctx.worker_active {
                    quit_pending = true;
                    let _ = ctx.bus.cancel().send(CancelEvent::Requested);
                    continue;
                }
                if ui_request.has_pending() || compaction.has_pending() {
                    quit_pending = true;
                    continue;
                }
                if pending_compaction_rx.is_some() {
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
    Ok(())
}

/// Bridge task: forwards blocking UI request responses to the event channel.
fn spawn_blocking_bridge(
    mut rx: mpsc::Receiver<UiRequestResponse>,
    tx: mpsc::Sender<OrchestratorEvent>,
) {
    tokio::spawn(async move {
        while let Some(resp) = rx.recv().await {
            let _ = tx.send(OrchestratorEvent::BlockingResponse(resp)).await;
        }
    });
}

/// Bridge task: forwards concurrent UI request responses to the event channel.
fn spawn_concurrent_bridge(
    mut rx: mpsc::Receiver<UiRequestResponse>,
    tx: mpsc::Sender<OrchestratorEvent>,
) {
    tokio::spawn(async move {
        while let Some(resp) = rx.recv().await {
            let _ = tx.send(OrchestratorEvent::ConcurrentResponse(resp)).await;
        }
    });
}

/// Bridge task: forwards compaction results to the event channel.
fn spawn_compaction_bridge(
    mut rx: mpsc::Receiver<Option<String>>,
    tx: mpsc::Sender<OrchestratorEvent>,
) {
    tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            let _ = tx
                .send(OrchestratorEvent::CompactionResult { message })
                .await;
        }
    });
}

/// Bus pump task: subscribes to the external bus channel and forwards
/// `ExternalEvent::PromptReceived` as `OrchestratorEvent::ExternalPrompt`.
fn spawn_bus_pump(bus: &Bus, tx: mpsc::Sender<OrchestratorEvent>) {
    let mut external_rx = bus.external().subscribe();
    tokio::spawn(async move {
        while let Ok(event) = external_rx.recv().await {
            match event {
                ExternalEvent::PromptReceived { prompt, task_id } => {
                    let _ = tx
                        .send(OrchestratorEvent::ExternalPrompt { prompt, task_id })
                        .await;
                }
            }
        }
    });
}

/// Bridge task: forwards A2A task cancellation IDs to the event channel.
/// Uses `spawn_blocking` because the source is a `std::mpsc::Receiver`.
fn spawn_task_cancel_bridge(
    rx: Option<std_mpsc::Receiver<String>>,
    tx: mpsc::Sender<OrchestratorEvent>,
) {
    if let Some(cancel_rx) = rx {
        tokio::task::spawn_blocking(move || {
            while let Ok(task_id) = cancel_rx.recv() {
                let _ = tx.blocking_send(OrchestratorEvent::ExternalCancel { task_id });
            }
        });
    }
}

#[cfg(test)]
#[path = "orchestrator_loop_test.rs"]
mod orchestrator_loop_test;
