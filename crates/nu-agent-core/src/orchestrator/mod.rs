pub mod pending;
pub mod poll;
pub mod pump;
pub mod router;
pub mod stages;
pub mod turn_outcome;

#[cfg(test)]
#[path = "pending_test.rs"]
mod pending_test;
#[cfg(test)]
#[path = "pump_test.rs"]
mod pump_test;
#[cfg(test)]
mod test;
#[cfg(test)]
mod test_shared;
#[cfg(test)]
mod turn_outcome_test;

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc as std_mpsc,
};
use tokio::sync::mpsc;

use nu_protocol::{LabeledError, Span, Value};

use crate::conversation::runtime::PendingPermissions;

use crate::orchestrator::{
    pump::EventPump,
    router::CommandRouter,
    stages::{OrchestrationContext, OrchestratorStages, StageOutcome},
    turn_outcome::TurnOutcome,
};
use crate::protocol::{
    compaction::CompactionTriggerSource,
    compaction_runtime::Compaction,
    contracts::{
        CoreRuntime, DisplayStateUi, LifecycleUi, McpUsabilityState, ProgressUi, TranscriptUi,
        UiMessageSnapshot, UserInputUi,
    },
    event::UiEvent,
    mcp_management::McpManagement,
    model_switching::ModelSwitching,
    session_management::{SessionPersistence, SessionState},
};

/// Configuration for the interactive loop.
///
/// Groups common arguments that would otherwise be passed individually,
/// keeping function signatures under clippy's `too_many_arguments` threshold.
pub struct InteractiveLoopConfig {
    /// The span to use for values created during the loop.
    pub span: Span,
    /// Pending permission requests awaiting user decisions.
    pub interactive_pending: Option<PendingPermissions>,
    /// Optional channel for receiving external prompts (e.g., A2A tasks).
    pub external_prompt_rx: Option<std_mpsc::Receiver<String>>,
    /// Optional sender for turn-complete notifications (prompt, response).
    pub on_turn_complete: Option<std_mpsc::Sender<(String, String)>>,
    /// Optional hydration config for resuming a prior session.
    pub hydration: Option<HydrationConfig>,
    /// Optional callback invoked after a successful agent switch.
    /// Receives the new agent's identity (name) and optional description.
    /// Used by the binary layer to update the A2A agent card.
    pub on_agent_switch: Option<OnAgentSwitch>,
}

impl InteractiveLoopConfig {
    /// Create a new config with the given span and all other fields set to `None`.
    pub fn new(span: Span) -> Self {
        Self {
            span,
            interactive_pending: None,
            external_prompt_rx: None,
            on_turn_complete: None,
            hydration: None,
            on_agent_switch: None,
        }
    }

    /// Set the interactive pending permissions.
    pub fn with_interactive_pending(mut self, pending: Option<PendingPermissions>) -> Self {
        self.interactive_pending = pending;
        self
    }

    /// Set the external prompt receiver.
    pub fn with_external_prompt_rx(mut self, rx: Option<std_mpsc::Receiver<String>>) -> Self {
        self.external_prompt_rx = rx;
        self
    }

    /// Set the on-turn-complete sender.
    pub fn with_on_turn_complete(mut self, tx: Option<std_mpsc::Sender<(String, String)>>) -> Self {
        self.on_turn_complete = tx;
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
pub(crate) type PendingMcpToggle = (String, std_mpsc::Receiver<McpToggleResult>);
pub type ModelSwitchResult = Result<(String, Option<u64>), String>;
pub type AgentSwitchResult = Result<(String, String, Option<u64>, Option<String>), String>;
pub type SessionSwitchResult = Result<Vec<UiMessageSnapshot>, String>;
pub(crate) type PendingModelSwitch = std_mpsc::Receiver<ModelSwitchResult>;
pub(crate) type PendingAgentSwitch = std_mpsc::Receiver<AgentSwitchResult>;
pub(crate) type PendingSessionSwitch = std_mpsc::Receiver<SessionSwitchResult>;
pub(crate) type PendingAutoCompaction = std_mpsc::Receiver<Option<String>>;
pub(crate) type PendingCompactionTrigger = std_mpsc::Receiver<Option<String>>;

/// Callback invoked after a successful agent switch.
/// Receives the new agent's identity (name), optional description, and optional icon.
pub type OnAgentSwitch = Arc<dyn Fn(String, Option<String>, Option<String>) + Send + Sync>;

pub enum WorkerCommand {
    ExecuteTurn {
        prompt: String,
        span: Span,
    },
    EvaluateAutoCompaction {
        response_tx: std_mpsc::Sender<Option<String>>,
    },
    ExecuteCompactionTrigger {
        source: CompactionTriggerSource,
        response_tx: std_mpsc::Sender<Option<String>>,
    },
    ToggleMcp {
        server_name: String,
        enable: bool,
        response_tx: std_mpsc::Sender<McpToggleResult>,
    },
    SwitchModel {
        model_spec: String,
        response_tx: std_mpsc::Sender<ModelSwitchResult>,
    },
    SwitchAgent {
        agent_name: String,
        response_tx: std_mpsc::Sender<AgentSwitchResult>,
    },
    SwitchSession {
        session_id: String,
        response_tx: std_mpsc::Sender<SessionSwitchResult>,
    },
    ClearSession,
    NewSession,
    Shutdown,
}

struct WorkerProgressUi {
    events: mpsc::UnboundedSender<UiEvent>,
    cancel_requested: Arc<AtomicBool>,
}

impl ProgressUi for WorkerProgressUi {
    fn emit(&mut self, event: &UiEvent) {
        let _ = self.events.send(event.clone());
    }

    fn flush(&mut self) {}

    fn take_cancel_requested(&self) -> bool {
        self.cancel_requested.swap(false, Ordering::SeqCst)
    }
}

/// Poll a one-shot option channel without blocking.
/// Returns (value, Some(rx)) if empty (re-park the receiver), (value, None) if disconnected.
/// The returned Option<T> is Some if a value was ready, None for all other outcomes
/// where the caller doesn't need the value.
///
/// Returns: (Option<T>, Option<Receiver<Option<T>>>)
///   - (Some(v), None)     — value ready
///   - (None, Some(rx))    — empty, re-park
///   - (None, None)        — disconnected
pub(crate) fn poll_option_channel<T>(
    rx: std_mpsc::Receiver<Option<T>>,
) -> (Option<T>, Option<std_mpsc::Receiver<Option<T>>>) {
    match rx.try_recv() {
        Ok(val) => (val, None),
        Err(std_mpsc::TryRecvError::Empty) => (None, Some(rx)),
        Err(std_mpsc::TryRecvError::Disconnected) => (None, None),
    }
}

pub(crate) async fn run_interactive_loop_impl<R, U>(
    mut runtime: R,
    ui: &mut U,
    config: InteractiveLoopConfig,
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
    U: ProgressUi + UserInputUi + DisplayStateUi + LifecycleUi + TranscriptUi,
{
    let InteractiveLoopConfig {
        span,
        interactive_pending,
        mut external_prompt_rx,
        on_turn_complete,
        hydration,
        ref on_agent_switch,
    } = config;

    if let Some(hydration) = hydration {
        ui.hydrate_transcript_from_messages(hydration.messages, hydration.last_total_tokens);
        runtime.seed_last_total_tokens(hydration.last_total_tokens);
    }

    let initial_visible_count = runtime.llm_visible_mcp_tool_count();
    ui.set_active_model_identity(runtime.active_model_identity().as_str());
    ui.set_context_window_max_tokens(runtime.max_context_tokens());
    for (server_name, names) in runtime.llm_visible_mcp_tool_names_by_server() {
        ui.set_mcp_visible_tool_count_by_server_name(&server_name, names.len());
        ui.set_mcp_visible_tool_names_by_server_name(&server_name, names);
    }

    let (worker_cmd_tx, mut worker_cmd_rx) = mpsc::channel::<WorkerCommand>(256);
    let (worker_event_tx, worker_event_rx) = mpsc::unbounded_channel::<UiEvent>();
    let (worker_result_tx, worker_result_rx) = mpsc::channel::<TurnOutcome>(256);
    let cancel_requested = Arc::new(AtomicBool::new(false));
    let worker_cancel = Arc::clone(&cancel_requested);

    let mut event_pump = EventPump::new(worker_event_rx);
    let mut stages = OrchestratorStages::new(initial_visible_count, worker_result_rx);
    let mut worker_active = false;
    let mut should_evaluate_compaction = true;
    let mut active_external_prompt: Option<String> = None;

    let mut worker_ui = WorkerProgressUi {
        events: worker_event_tx,
        cancel_requested: worker_cancel,
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

    // Main loop: pump UI, drain events, run stages, check quit conditions.
    loop {
        ui.pump_once();
        if let Some(error) = ui.fatal_error() {
            let _ = worker_cmd_tx.send(WorkerCommand::Shutdown).await;
            let runtime = worker_handle
                .await
                .unwrap_or_else(|_| panic!("worker panicked"));
            return (
                runtime,
                Err(LabeledError::new(format!("Interactive UI failed: {error}"))),
            );
        }
        if ui.take_cancel_requested() {
            cancel_requested.store(true, Ordering::SeqCst);
        }
        event_pump.drain_batch(ui);

        // Check for external prompts (e.g., A2A tasks) when worker is idle.
        if !worker_active
            && let Some(ref mut ext_rx) = external_prompt_rx
            && let Ok(prompt) = ext_rx.try_recv()
        {
            log::info!(
                "orchestrator: dispatching external prompt (len={})",
                prompt.len()
            );
            ui.display_incoming_message(&prompt);
            active_external_prompt = Some(prompt.clone());

            worker_cmd_tx
                .send(WorkerCommand::ExecuteTurn { prompt, span })
                .await
                .unwrap_or(());
            worker_active = true;
            continue;
        }

        let mut ctx = OrchestrationContext {
            worker_tx: &worker_cmd_tx,
            pending: &interactive_pending,
            worker_active: &mut worker_active,
            should_evaluate_compaction: &mut should_evaluate_compaction,
            span,
            ui,
            active_external_prompt: &mut active_external_prompt,
            on_turn_complete: &on_turn_complete,
        };
        match stages.poll_all(&mut ctx).await {
            StageOutcome::Fatal(e) => {
                let _ = worker_cmd_tx.send(WorkerCommand::Shutdown).await;
                let runtime = worker_handle
                    .await
                    .unwrap_or_else(|_| panic!("worker panicked"));
                return (runtime, Err(e));
            }
            StageOutcome::Handled => continue,
            StageOutcome::Idle => {
                // Yield to allow the worker task to make progress.
                // Without this, the main loop spins in a tight loop and
                // the spawned worker never gets a chance to process commands.
                tokio::task::yield_now().await;
            }
        }
        if !ui.quit_requested() {
            continue;
        }
        if worker_active {
            cancel_requested.store(true, Ordering::SeqCst);
            continue;
        }
        if stages.has_pending_ops() || should_evaluate_compaction {
            continue;
        }
        break;
    }

    let _ = worker_cmd_tx.send(WorkerCommand::Shutdown).await;
    let runtime = worker_handle
        .await
        .unwrap_or_else(|_| panic!("worker panicked"));

    (runtime, Ok(Value::nothing(span)))
}

pub async fn run_interactive_loop<R, U>(
    runtime: R,
    ui: &mut U,
    config: InteractiveLoopConfig,
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
    U: ProgressUi + UserInputUi + DisplayStateUi + LifecycleUi + TranscriptUi,
{
    let (_runtime, result) = run_interactive_loop_impl(runtime, ui, config).await;
    result
}

/// Like [`run_interactive_loop`] but also checks an external channel for
/// pre-formatted prompt strings (e.g., injected A2A tasks) before each
/// iteration. These prompts are dispatched with higher priority than
/// regular user input.
///
/// The `on_turn_complete` callback is fired after each turn that was triggered
/// by an external prompt, with `(prompt_text, response_text)`.
pub async fn run_interactive_loop_with_external_prompts<R, U>(
    runtime: R,
    ui: &mut U,
    config: InteractiveLoopConfig,
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
    U: ProgressUi + UserInputUi + DisplayStateUi + LifecycleUi + TranscriptUi,
{
    let (_runtime, result) = run_interactive_loop_impl(runtime, ui, config).await;
    result
}

pub async fn run_hydrated_interactive_loop<R, U>(
    runtime: R,
    ui: &mut U,
    config: InteractiveLoopConfig,
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
    U: ProgressUi + UserInputUi + DisplayStateUi + LifecycleUi + TranscriptUi,
{
    let (_runtime, result) = run_interactive_loop_impl(runtime, ui, config).await;
    result
}

/// Like [`run_hydrated_interactive_loop`] but also checks an external channel
/// for pre-formatted prompt strings (e.g., injected A2A tasks) before each
/// iteration.
///
/// The `on_turn_complete` callback is fired after each turn that was triggered
/// by an external prompt, with `(prompt_text, response_text)`.
pub async fn run_hydrated_interactive_loop_with_external_prompts<R, U>(
    runtime: R,
    ui: &mut U,
    config: InteractiveLoopConfig,
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
    U: ProgressUi + UserInputUi + DisplayStateUi + LifecycleUi + TranscriptUi,
{
    let (_runtime, result) = run_interactive_loop_impl(runtime, ui, config).await;
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
