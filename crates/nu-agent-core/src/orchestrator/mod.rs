pub mod pending;
pub mod poll;
pub mod pump;
pub mod router;
pub mod stages;
pub mod turn_outcome;

#[cfg(test)]
mod compaction_test;
#[cfg(test)]
mod formatting_test;
#[cfg(test)]
mod lifecycle_test;
#[cfg(test)]
mod mcp_toggle_test;
#[cfg(test)]
mod model_switch_test;
#[cfg(test)]
#[path = "pending_test.rs"]
mod pending_test;
#[cfg(test)]
#[path = "pump_test.rs"]
mod pump_test;
#[cfg(test)]
mod test_shared;
#[cfg(test)]
mod turn_outcome_test;

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

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
    compaction_runtime::HasCompaction,
    contracts::{
        CoreRuntime, DisplayStateUi, LifecycleUi, McpUsabilityState, ProgressUi, TranscriptUi,
        UiMessageSnapshot, UserInputUi,
    },
    event::UiEvent,
    mcp_management::HasMcpManagement,
    model_switching::HasModelSwitching,
    session_management::HasSessionManagement,
};

pub type McpToggleResult = (
    Result<McpUsabilityState, String>,
    usize,
    usize,
    Vec<(String, Vec<String>)>,
);
pub(crate) type PendingMcpToggle = (String, mpsc::Receiver<McpToggleResult>);
pub type ModelSwitchResult = Result<(String, Option<u64>), String>;
pub type AgentSwitchResult = Result<(String, String, Option<u64>), String>;
pub(crate) type PendingModelSwitch = mpsc::Receiver<ModelSwitchResult>;
pub(crate) type PendingAgentSwitch = mpsc::Receiver<AgentSwitchResult>;
pub(crate) type PendingAutoCompaction = mpsc::Receiver<Option<String>>;
pub(crate) type PendingCompactionTrigger = mpsc::Receiver<Option<String>>;

pub enum WorkerCommand {
    ExecuteTurn {
        prompt: String,
        span: Span,
    },
    EvaluateAutoCompaction {
        response_tx: mpsc::Sender<Option<String>>,
    },
    ExecuteCompactionTrigger {
        source: CompactionTriggerSource,
        response_tx: mpsc::Sender<Option<String>>,
    },
    ToggleMcp {
        server_name: String,
        enable: bool,
        response_tx: mpsc::Sender<McpToggleResult>,
    },
    SwitchModel {
        model_spec: String,
        response_tx: mpsc::Sender<ModelSwitchResult>,
    },
    SwitchAgent {
        agent_name: String,
        response_tx: mpsc::Sender<AgentSwitchResult>,
    },
    ClearSession,
    NewSession,
    Shutdown,
}

struct WorkerProgressUi {
    events: mpsc::Sender<UiEvent>,
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
    rx: mpsc::Receiver<Option<T>>,
) -> (Option<T>, Option<mpsc::Receiver<Option<T>>>) {
    match rx.try_recv() {
        Ok(val) => (val, None),
        Err(mpsc::TryRecvError::Empty) => (None, Some(rx)),
        Err(mpsc::TryRecvError::Disconnected) => (None, None),
    }
}

fn run_interactive_loop_impl<R, U>(
    runtime: &mut R,
    ui: &mut U,
    span: Span,
    interactive_pending: Option<PendingPermissions>,
    external_prompt_rx: Option<mpsc::Receiver<String>>,
    on_turn_complete: Option<mpsc::Sender<(String, String)>>,
) -> Result<Value, LabeledError>
where
    R: CoreRuntime
        + HasMcpManagement
        + HasModelSwitching
        + HasSessionManagement
        + HasCompaction
        + Send,
    U: ProgressUi + UserInputUi + DisplayStateUi + LifecycleUi + TranscriptUi,
{
    let initial_visible_count = runtime.llm_visible_mcp_tool_count();
    ui.set_active_model_identity(runtime.active_model_identity().as_str());
    ui.set_context_window_max_tokens(runtime.max_context_tokens());
    for (server_name, names) in runtime.llm_visible_mcp_tool_names_by_server() {
        ui.set_mcp_visible_tool_count_by_server_name(&server_name, names.len());
        ui.set_mcp_visible_tool_names_by_server_name(&server_name, names);
    }

    std::thread::scope(|scope| {
        let (worker_cmd_tx, worker_cmd_rx) = mpsc::channel::<WorkerCommand>();
        let (worker_event_tx, worker_event_rx) = mpsc::channel::<UiEvent>();
        let (worker_result_tx, worker_result_rx) = mpsc::channel::<TurnOutcome>();
        let cancel_requested = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel_requested);

        let worker = scope.spawn(move || {
            let mut worker_ui = WorkerProgressUi {
                events: worker_event_tx,
                cancel_requested: worker_cancel,
            };
            let mut router = CommandRouter::new(runtime);

            while let Ok(command) = worker_cmd_rx.recv() {
                if !router.dispatch(command, &mut worker_ui, &worker_result_tx) {
                    break;
                }
            }
        });

        let mut event_pump = EventPump::new(worker_event_rx);
        let mut stages = OrchestratorStages::new(initial_visible_count, worker_result_rx);
        let mut worker_active = false;
        let mut should_evaluate_compaction = true; // evaluate once on startup (session resume)
        let mut active_external_prompt: Option<String> = None;

        loop {
            ui.pump_once();
            if let Some(error) = ui.fatal_error() {
                let _ = worker_cmd_tx.send(WorkerCommand::Shutdown);
                return Err(LabeledError::new(format!("Interactive UI failed: {error}")));
            }
            if ui.take_cancel_requested() {
                cancel_requested.store(true, Ordering::SeqCst);
            }
            event_pump.drain_batch(ui);

            // Check for external prompts (e.g., A2A tasks) when worker is idle.
            // This runs BEFORE the stage pipeline so external inputs take priority
            // over regular user input.
            if !worker_active
                && let Some(ref ext_rx) = external_prompt_rx
                && let Ok(prompt) = ext_rx.try_recv()
            {
                log::info!(
                    "orchestrator: dispatching external prompt (len={})",
                    prompt.len()
                );

                // Display the prompt as a user message in the TUI transcript
                // so the user can see what was sent to the receiving agent.
                ui.display_incoming_message(&prompt);

                // Track this external prompt so the session stage can fire
                // on_turn_complete after the LLM finishes processing it.
                active_external_prompt = Some(prompt.clone());

                match worker_cmd_tx.send(WorkerCommand::ExecuteTurn { prompt, span }) {
                    Ok(()) => {
                        worker_active = true;
                        continue;
                    }
                    Err(_) => {
                        let _ = worker_cmd_tx.send(WorkerCommand::Shutdown);
                        return Err(LabeledError::new(
                            "Worker channel closed while dispatching external prompt",
                        ));
                    }
                }
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
            match stages.poll_all(&mut ctx) {
                StageOutcome::Fatal(e) => {
                    let _ = worker_cmd_tx.send(WorkerCommand::Shutdown);
                    return Err(e);
                }
                StageOutcome::Handled => continue,
                StageOutcome::Idle => {}
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

        let _ = worker_cmd_tx.send(WorkerCommand::Shutdown);
        let _ = worker;

        Ok(Value::nothing(span))
    })
}

pub fn run_interactive_loop<R, U>(
    runtime: &mut R,
    ui: &mut U,
    span: Span,
    interactive_pending: Option<PendingPermissions>,
) -> Result<Value, LabeledError>
where
    R: CoreRuntime
        + HasMcpManagement
        + HasModelSwitching
        + HasSessionManagement
        + HasCompaction
        + Send,
    U: ProgressUi + UserInputUi + DisplayStateUi + LifecycleUi + TranscriptUi,
{
    run_interactive_loop_impl(runtime, ui, span, interactive_pending, None, None)
}

/// Like [`run_interactive_loop`] but also checks an external channel for
/// pre-formatted prompt strings (e.g., injected A2A tasks) before each
/// iteration. These prompts are dispatched with higher priority than
/// regular user input.
///
/// The `on_turn_complete` callback is fired after each turn that was triggered
/// by an external prompt, with `(prompt_text, response_text)`.
#[allow(clippy::too_many_arguments)]
pub fn run_interactive_loop_with_external_prompts<R, U>(
    runtime: &mut R,
    ui: &mut U,
    span: Span,
    interactive_pending: Option<PendingPermissions>,
    external_prompt_rx: Option<mpsc::Receiver<String>>,
    on_turn_complete: Option<mpsc::Sender<(String, String)>>,
) -> Result<Value, LabeledError>
where
    R: CoreRuntime
        + HasMcpManagement
        + HasModelSwitching
        + HasSessionManagement
        + HasCompaction
        + Send,
    U: ProgressUi + UserInputUi + DisplayStateUi + LifecycleUi + TranscriptUi,
{
    run_interactive_loop_impl(
        runtime,
        ui,
        span,
        interactive_pending,
        external_prompt_rx,
        on_turn_complete,
    )
}

pub fn run_hydrated_interactive_loop<R, U>(
    runtime: &mut R,
    ui: &mut U,
    messages: impl IntoIterator<Item = UiMessageSnapshot>,
    last_total_tokens: Option<u64>,
    span: Span,
    interactive_pending: Option<PendingPermissions>,
) -> Result<Value, LabeledError>
where
    R: CoreRuntime
        + HasMcpManagement
        + HasModelSwitching
        + HasSessionManagement
        + HasCompaction
        + Send,
    U: ProgressUi + UserInputUi + DisplayStateUi + LifecycleUi + TranscriptUi,
{
    run_hydrated_interactive_loop_impl(
        runtime,
        ui,
        messages,
        last_total_tokens,
        span,
        interactive_pending,
        None,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_hydrated_interactive_loop_impl<R, U>(
    runtime: &mut R,
    ui: &mut U,
    messages: impl IntoIterator<Item = UiMessageSnapshot>,
    last_total_tokens: Option<u64>,
    span: Span,
    interactive_pending: Option<PendingPermissions>,
    external_prompt_rx: Option<mpsc::Receiver<String>>,
    on_turn_complete: Option<mpsc::Sender<(String, String)>>,
) -> Result<Value, LabeledError>
where
    R: CoreRuntime
        + HasMcpManagement
        + HasModelSwitching
        + HasSessionManagement
        + HasCompaction
        + Send,
    U: ProgressUi + UserInputUi + DisplayStateUi + LifecycleUi + TranscriptUi,
{
    ui.hydrate_transcript_from_messages(messages, last_total_tokens);
    runtime.seed_last_total_tokens(last_total_tokens);
    run_interactive_loop_impl(
        runtime,
        ui,
        span,
        interactive_pending,
        external_prompt_rx,
        on_turn_complete,
    )
}

/// Like [`run_hydrated_interactive_loop`] but also checks an external channel
/// for pre-formatted prompt strings (e.g., injected A2A tasks) before each
/// iteration.
///
/// The `on_turn_complete` callback is fired after each turn that was triggered
/// by an external prompt, with `(prompt_text, response_text)`.
#[allow(clippy::too_many_arguments)]
pub fn run_hydrated_interactive_loop_with_external_prompts<R, U>(
    runtime: &mut R,
    ui: &mut U,
    messages: impl IntoIterator<Item = UiMessageSnapshot>,
    last_total_tokens: Option<u64>,
    span: Span,
    interactive_pending: Option<PendingPermissions>,
    external_prompt_rx: Option<mpsc::Receiver<String>>,
    on_turn_complete: Option<mpsc::Sender<(String, String)>>,
) -> Result<Value, LabeledError>
where
    R: CoreRuntime
        + HasMcpManagement
        + HasModelSwitching
        + HasSessionManagement
        + HasCompaction
        + Send,
    U: ProgressUi + UserInputUi + DisplayStateUi + LifecycleUi + TranscriptUi,
{
    run_hydrated_interactive_loop_impl(
        runtime,
        ui,
        messages,
        last_total_tokens,
        span,
        interactive_pending,
        external_prompt_rx,
        on_turn_complete,
    )
}

pub fn run_single_turn<R, U>(
    runtime: &mut R,
    ui: &mut U,
    prompt: String,
    context: Option<String>,
    span: Span,
) -> Result<Value, LabeledError>
where
    R: CoreRuntime,
    U: ProgressUi,
{
    runtime.execute_turn(ui, prompt, context, span)
}
