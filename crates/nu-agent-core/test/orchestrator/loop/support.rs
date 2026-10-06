//! Shared fixtures for the orchestrator loop test modules.
//!
//! Provides the mock stage implementations, the `Harness` channel bundle, and
//! the A2A task helpers used by every orchestrator loop scenario file.

use nu_agent_a2a::{
    A2aCompletionEvent, InMemoryTaskStore, IncomingTask, Message, Part, Role, TaskState,
};
use nu_protocol::Span;
use tokio::sync::mpsc;

use crate::bus::{Bus, CompactionRx, ExternalRx, create_bus};
use crate::conversation::runtime::PendingPermissions;
use crate::orchestrator::stages::{
    OrchestrationContext, PermissionHandler, SessionHandler, SlashHandler, UiRequestHandler,
};
use crate::orchestrator::turn_outcome::TurnOutcome;
use crate::orchestrator::{
    OrchestratorEvent, SourceChannels, UiRequest, UiRequestResponse, WorkerCommand,
};
use crate::protocol::event::PermissionDecisionSubmission;

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// Mock stage implementations
// ---------------------------------------------------------------------------

pub(super) struct MockSlash {
    pub(super) handle_calls: Vec<String>,
}

impl MockSlash {
    pub(super) fn new() -> Self {
        Self {
            handle_calls: Vec::new(),
        }
    }
}

impl SlashHandler for MockSlash {
    async fn handle(&mut self, prompt: String, _ctx: &mut OrchestrationContext<'_>) {
        self.handle_calls.push(prompt);
    }
}

pub(super) struct MockPermission {
    pub(super) handle_calls: usize,
}

impl MockPermission {
    pub(super) fn new() -> Self {
        Self { handle_calls: 0 }
    }
}

impl PermissionHandler for MockPermission {
    fn handle(&mut self, _decision: PermissionDecisionSubmission, _ctx: &mut OrchestrationContext) {
        self.handle_calls += 1;
    }
}

pub(super) struct MockUiRequest {
    pub(super) handle_incoming_calls: Vec<UiRequest>,
    pub(super) drain_queued_calls: usize,
    pub(super) blocking_pending: bool,
    pub(super) pending: bool,
}

impl MockUiRequest {
    pub(super) fn new() -> Self {
        Self {
            handle_incoming_calls: Vec::new(),
            drain_queued_calls: 0,
            blocking_pending: false,
            pending: false,
        }
    }
}

impl UiRequestHandler for MockUiRequest {
    async fn handle_incoming(&mut self, request: UiRequest, _ctx: &mut OrchestrationContext<'_>) {
        self.handle_incoming_calls.push(request);
    }

    async fn handle_blocking_response(
        &mut self,
        _response: UiRequestResponse,
        _ctx: &mut OrchestrationContext<'_>,
    ) {
    }

    async fn handle_concurrent_response(
        &mut self,
        _response: UiRequestResponse,
        _ctx: &mut OrchestrationContext<'_>,
    ) {
    }

    async fn drain_queued(&mut self, _ctx: &mut OrchestrationContext<'_>) {
        self.drain_queued_calls += 1;
    }

    fn has_blocking_pending(&self) -> bool {
        self.blocking_pending
    }

    fn has_pending(&self) -> bool {
        self.pending
    }
}

pub(super) struct MockSession {
    pub(super) handle_outcome_calls: Vec<TurnOutcome>,
}

impl MockSession {
    pub(super) fn new() -> Self {
        Self {
            handle_outcome_calls: Vec::new(),
        }
    }
}

impl SessionHandler for MockSession {
    async fn handle_outcome(&mut self, outcome: TurnOutcome, ctx: &mut OrchestrationContext<'_>) {
        *ctx.worker_active = false;
        self.handle_outcome_calls.push(outcome);
    }
}

// ---------------------------------------------------------------------------
// Test harness
// ---------------------------------------------------------------------------

/// Mutable per-test state that `run_orchestrator_loop` reads and writes
/// through `OrchestrationContext`. Kept separate from the channels so `ctx()`
/// can borrow it disjointly from the event channels, worker, and stage state.
pub(crate) struct CtxState {
    pub(crate) worker_active: bool,
    pub(crate) pending: Option<PendingPermissions>,
    pub(crate) active_external_prompt: Option<String>,
    pub(crate) active_external_task_id: Option<String>,
    pub(crate) pending_external_cancel: Option<String>,
    pub(crate) pending_a2a_task_id: Option<String>,
    pub(crate) task_store: Option<std::sync::Arc<InMemoryTaskStore>>,
}

pub(crate) struct HarnessParts<'a> {
    pub(crate) event_tx: &'a mpsc::Sender<OrchestratorEvent>,
    pub(crate) event_rx: &'a mut mpsc::Receiver<OrchestratorEvent>,
    pub(crate) worker_tx: &'a mpsc::Sender<WorkerCommand>,
    pub(crate) worker_rx: &'a mut Option<mpsc::Receiver<WorkerCommand>>,
    pub(crate) worker_result_tx: &'a mpsc::Sender<TurnOutcome>,
    pub(crate) worker_result_rx: &'a mut mpsc::Receiver<TurnOutcome>,
    pub(crate) blocking_tx: &'a mpsc::Sender<UiRequestResponse>,
    pub(crate) blocking_response_rx: &'a mut mpsc::Receiver<UiRequestResponse>,
    pub(crate) concurrent_tx: &'a mpsc::Sender<UiRequestResponse>,
    pub(crate) concurrent_response_rx: &'a mut mpsc::Receiver<UiRequestResponse>,
    pub(crate) external_rx: &'a mut ExternalRx,
    pub(crate) compaction_rx: &'a mut CompactionRx,
    pub(crate) task_cancel_rx: &'a mut Option<mpsc::UnboundedReceiver<String>>,
    pub(crate) bus: &'a Bus,
    pub(crate) state: &'a mut CtxState,
}

pub(crate) struct Harness {
    pub(crate) event_tx: mpsc::Sender<OrchestratorEvent>,
    pub(crate) event_rx: mpsc::Receiver<OrchestratorEvent>,
    pub(crate) worker_tx: mpsc::Sender<WorkerCommand>,
    pub(crate) worker_rx: Option<mpsc::Receiver<WorkerCommand>>,
    pub(crate) worker_result_tx: mpsc::Sender<TurnOutcome>,
    pub(crate) worker_result_rx: mpsc::Receiver<TurnOutcome>,
    pub(crate) blocking_tx: mpsc::Sender<UiRequestResponse>,
    pub(crate) blocking_response_rx: mpsc::Receiver<UiRequestResponse>,
    pub(crate) concurrent_tx: mpsc::Sender<UiRequestResponse>,
    pub(crate) concurrent_response_rx: mpsc::Receiver<UiRequestResponse>,
    pub(crate) external_rx: ExternalRx,
    pub(crate) compaction_rx: CompactionRx,
    pub(crate) task_cancel_rx: Option<mpsc::UnboundedReceiver<String>>,
    pub(crate) bus: Bus,
    pub(crate) ctx_state: CtxState,
}

impl Harness {
    pub(crate) fn new() -> Self {
        let (event_tx, event_rx) = mpsc::channel::<OrchestratorEvent>(256);
        let (worker_tx, worker_rx) = mpsc::channel::<WorkerCommand>(256);
        let (worker_result_tx, worker_result_rx) = mpsc::channel::<TurnOutcome>(256);
        let (blocking_tx, blocking_response_rx) = mpsc::channel::<UiRequestResponse>(256);
        let (concurrent_tx, concurrent_response_rx) = mpsc::channel::<UiRequestResponse>(256);
        let bus = create_bus();
        let external_rx = bus.external().subscribe();
        let compaction_rx = bus.compaction().subscribe();
        let (_cancel_tx, cancel_rx) = mpsc::unbounded_channel::<String>();
        Self {
            event_tx,
            event_rx,
            worker_tx,
            worker_rx: Some(worker_rx),
            worker_result_tx,
            worker_result_rx,
            blocking_tx,
            blocking_response_rx,
            concurrent_tx,
            concurrent_response_rx,
            external_rx,
            compaction_rx,
            task_cancel_rx: Some(cancel_rx),
            bus,
            ctx_state: CtxState {
                worker_active: false,
                pending: None,
                active_external_prompt: None,
                active_external_task_id: None,
                pending_external_cancel: None,
                pending_a2a_task_id: None,
                task_store: None,
            },
        }
    }

    pub(crate) fn parts(&mut self) -> HarnessParts<'_> {
        HarnessParts {
            event_tx: &self.event_tx,
            event_rx: &mut self.event_rx,
            worker_tx: &self.worker_tx,
            worker_rx: &mut self.worker_rx,
            worker_result_tx: &self.worker_result_tx,
            worker_result_rx: &mut self.worker_result_rx,
            blocking_tx: &self.blocking_tx,
            blocking_response_rx: &mut self.blocking_response_rx,
            concurrent_tx: &self.concurrent_tx,
            concurrent_response_rx: &mut self.concurrent_response_rx,
            external_rx: &mut self.external_rx,
            compaction_rx: &mut self.compaction_rx,
            task_cancel_rx: &mut self.task_cancel_rx,
            bus: &self.bus,
            state: &mut self.ctx_state,
        }
    }
}

pub(crate) fn make_ctx<'a>(
    worker_tx: &'a mpsc::Sender<WorkerCommand>,
    blocking_tx: &'a mpsc::Sender<UiRequestResponse>,
    concurrent_tx: &'a mpsc::Sender<UiRequestResponse>,
    bus: &'a Bus,
    state: &'a mut CtxState,
) -> OrchestrationContext<'a> {
    OrchestrationContext {
        worker_tx,
        blocking_response_tx: blocking_tx,
        concurrent_response_tx: concurrent_tx,
        pending: &state.pending,
        worker_active: &mut state.worker_active,
        span: Span::test_data(),
        active_external_prompt: &mut state.active_external_prompt,
        active_external_task_id: &mut state.active_external_task_id,
        pending_external_cancel: &mut state.pending_external_cancel,
        pending_a2a_task_id: &mut state.pending_a2a_task_id,
        bus,
        task_store: state.task_store.as_ref(),
    }
}

pub(super) fn take_event_rx(
    event_rx: &mut mpsc::Receiver<OrchestratorEvent>,
) -> mpsc::Receiver<OrchestratorEvent> {
    let (_, placeholder) = mpsc::channel::<OrchestratorEvent>(256);
    std::mem::replace(event_rx, placeholder)
}

pub(super) fn recv_command(
    worker_rx: &mut Option<mpsc::Receiver<WorkerCommand>>,
) -> Option<WorkerCommand> {
    worker_rx.as_mut()?.try_recv().ok()
}

/// Take an owned mpsc receiver out of a `&mut` slot, leaving a placeholder.
fn take_rx<T>(rx: &mut mpsc::Receiver<T>) -> mpsc::Receiver<T> {
    let (_, placeholder) = mpsc::channel::<T>(256);
    std::mem::replace(rx, placeholder)
}

/// Build the `SourceChannels` for `run_orchestrator_loop` from the harness
/// parts. The owned mpsc receivers are taken out of the harness; the broadcast
/// receivers are passed by mutable reference.
pub(super) fn make_sources<'a>(
    worker_result_rx: &'a mut mpsc::Receiver<TurnOutcome>,
    blocking_response_rx: &'a mut mpsc::Receiver<UiRequestResponse>,
    concurrent_response_rx: &'a mut mpsc::Receiver<UiRequestResponse>,
    external_rx: &'a mut ExternalRx,
    compaction_rx: &'a mut CompactionRx,
    task_cancel_rx: &'a mut Option<mpsc::UnboundedReceiver<String>>,
) -> SourceChannels<'a> {
    SourceChannels {
        worker_result_rx: take_rx(worker_result_rx),
        blocking_response_rx: take_rx(blocking_response_rx),
        concurrent_response_rx: take_rx(concurrent_response_rx),
        external_rx,
        compaction_rx,
        task_cancel_rx: task_cancel_rx.take(),
        a2a_task_rx: None,
        a2a_completion_rx: None,
    }
}

/// Receive the next `WorkerCommand` with a timeout, or fail with `what`.
pub(super) async fn recv_worker_command(
    worker_rx: &mut Option<mpsc::Receiver<WorkerCommand>>,
    what: &str,
) -> Result<WorkerCommand> {
    let rx = worker_rx.as_mut().ok_or("worker_rx present")?;
    let cmd = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .map_err(|_| format!("{what} should be dispatched"))?
        .ok_or("worker channel should not close")?;
    Ok(cmd)
}

/// Build an `IncomingTask` with a single text part and no `contextId`.
pub(super) fn incoming_task(task_id: &str, text: &str) -> IncomingTask {
    IncomingTask {
        task_id: task_id.to_string(),
        message: Message {
            role: Role::User,
            parts: vec![Part::Text {
                text: text.to_string(),
            }],
            message_id: format!("msg-{task_id}"),
            extensions: None,
            metadata: None,
        },
        sender_url: "http://a.local".to_string(),
        context_id: None,
        parent_task_id: None,
    }
}

/// Build a completed `A2aCompletionEvent` with no `contextId`.
pub(super) fn completion_event(task_id: &str, result: &str) -> A2aCompletionEvent {
    A2aCompletionEvent {
        task_id: task_id.to_string(),
        agent_name: "agent-b".to_string(),
        result: result.to_string(),
        status: TaskState::Completed,
        context_id: None,
    }
}

/// Poll the store until `task_id` reaches `expected`, or fail after 5 seconds.
pub(super) async fn wait_for_task_state(
    store: &InMemoryTaskStore,
    task_id: &str,
    expected: TaskState,
) -> Result<()> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let state = store
            .get_task(task_id)
            .map_err(|e| format!("task {task_id} should exist: {e}"))?
            .status
            .state;
        if state == expected {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            return Err(format!("task {task_id} should reach {expected:?}, got {state:?}").into());
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
}
