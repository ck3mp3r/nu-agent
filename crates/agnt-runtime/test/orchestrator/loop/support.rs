//! Shared fixtures for the orchestrator loop test modules.
//!
//! Provides the mock stage implementations, the owned `Harness` fixture, and
//! the external task fixtures used by every orchestrator loop scenario file.

use std::sync::Mutex;

use nu_protocol::Span;
use tokio::sync::mpsc;

use crate::bus::{Bus, ControlRx, LifecycleRx};
use crate::catalog::PickerCatalog;
use crate::conversation::runtime::PendingPermissions;
use crate::orchestrator::SourceChannels;
use crate::orchestrator::events::{OrchestratorEvent, UiRequest, UiRequestResponse, WorkerCommand};
use crate::orchestrator::external::{ExternalCompletion, ExternalTask, ExternalTaskStore};
pub(crate) use crate::orchestrator::loop_impl::WORKER_CMD_CAPACITY;
use crate::orchestrator::stages::{
    OrchestrationContext, PermissionHandler, SessionHandler, SlashHandler, UiRequestHandler,
};
use crate::orchestrator::turn_outcome::TurnResult;
use crate::protocol::event::PermissionDecisionSubmission;

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// An ordered trace of the stage calls a test observes. The mocks append to it
/// so a test can assert the order in which the loop reached them.
pub(super) type Trace = std::sync::Arc<std::sync::Mutex<Vec<String>>>;

// ---------------------------------------------------------------------------
// Mock stage implementations
// ---------------------------------------------------------------------------

pub(super) struct MockSlash {
    pub(super) handle_calls: Vec<String>,
    pub(super) trace: Trace,
}

impl MockSlash {
    pub(super) fn new() -> Self {
        Self {
            handle_calls: Vec::new(),
            trace: Trace::default(),
        }
    }

    pub(super) fn with_trace(trace: Trace) -> Self {
        Self {
            handle_calls: Vec::new(),
            trace,
        }
    }
}

impl SlashHandler for MockSlash {
    async fn handle(
        &mut self,
        prompt: String,
        _task_id: Option<String>,
        _ctx: &mut OrchestrationContext<'_>,
    ) {
        self.trace
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(format!("slash:{prompt}"));
        self.handle_calls.push(prompt);
    }
}

pub(super) struct MockPermission {
    pub(super) handle_calls: usize,
    pub(super) trace: Trace,
}

impl MockPermission {
    pub(super) fn new() -> Self {
        Self {
            handle_calls: 0,
            trace: Trace::default(),
        }
    }

    pub(super) fn with_trace(trace: Trace) -> Self {
        Self {
            handle_calls: 0,
            trace,
        }
    }
}

impl PermissionHandler for MockPermission {
    async fn handle(
        &mut self,
        _decision: PermissionDecisionSubmission,
        _ctx: &mut OrchestrationContext<'_>,
    ) {
        self.handle_calls += 1;
        if let Ok(mut trace) = self.trace.lock() {
            trace.push("permission".to_string());
        }
    }
}

pub(super) struct MockUiRequest {
    pub(super) handle_incoming_calls: Vec<UiRequest>,
    pub(super) drain_queued_calls: usize,
    pub(super) blocking_pending: bool,
    pub(super) pending: bool,
    pub(super) trace: Trace,
}

impl MockUiRequest {
    pub(super) fn new() -> Self {
        Self {
            handle_incoming_calls: Vec::new(),
            drain_queued_calls: 0,
            blocking_pending: false,
            pending: false,
            trace: Trace::default(),
        }
    }

    pub(super) fn with_trace(trace: Trace) -> Self {
        Self {
            handle_incoming_calls: Vec::new(),
            drain_queued_calls: 0,
            blocking_pending: false,
            pending: false,
            trace,
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
        self.trace
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push("blocking_response".to_string());
        self.blocking_pending = false;
        self.pending = false;
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
    pub(super) handle_outcome_calls: Vec<TurnResult>,
}

impl MockSession {
    pub(super) fn new() -> Self {
        Self {
            handle_outcome_calls: Vec::new(),
        }
    }
}

impl SessionHandler for MockSession {
    async fn handle_outcome(&mut self, outcome: TurnResult, ctx: &mut OrchestrationContext<'_>) {
        *ctx.worker_active = false;
        self.handle_outcome_calls.push(outcome);
    }
}

// ---------------------------------------------------------------------------
// Test harness
// ---------------------------------------------------------------------------

/// Mutable per-test state that `run_orchestrator_loop` reads and writes
/// through `OrchestrationContext`. Kept separate from the channels so
/// `make_ctx` can borrow it disjointly from the event channels, worker, and
/// stage state.
pub(crate) struct CtxState {
    pub(crate) worker_active: bool,
    pub(crate) catalog: PickerCatalog,
    pub(crate) pending: Option<PendingPermissions>,
}

pub(crate) struct Harness {
    pub(crate) event_tx: mpsc::Sender<OrchestratorEvent>,
    pub(crate) event_rx: mpsc::Receiver<OrchestratorEvent>,
    pub(crate) worker_tx: mpsc::Sender<WorkerCommand>,
    pub(crate) worker_rx: Option<mpsc::Receiver<WorkerCommand>>,
    pub(crate) worker_result_tx: mpsc::Sender<TurnResult>,
    pub(crate) worker_result_rx: mpsc::Receiver<TurnResult>,
    pub(crate) blocking_tx: mpsc::Sender<UiRequestResponse>,
    pub(crate) blocking_response_rx: mpsc::Receiver<UiRequestResponse>,
    pub(crate) concurrent_tx: mpsc::Sender<UiRequestResponse>,
    pub(crate) concurrent_response_rx: mpsc::Receiver<UiRequestResponse>,
    pub(crate) control_rx: ControlRx,
    pub(crate) lifecycle_rx: LifecycleRx,
    pub(crate) task_cancel_rx: Option<mpsc::UnboundedReceiver<String>>,
    pub(crate) bus: Bus,
    pub(crate) ctx_state: CtxState,
}

impl Harness {
    pub(crate) fn new() -> Self {
        let (event_tx, event_rx) = mpsc::channel::<OrchestratorEvent>(256);
        let (worker_tx, worker_rx) = mpsc::channel::<WorkerCommand>(WORKER_CMD_CAPACITY);
        let (worker_result_tx, worker_result_rx) = mpsc::channel::<TurnResult>(256);
        let (blocking_tx, blocking_response_rx) = mpsc::channel::<UiRequestResponse>(256);
        let (concurrent_tx, concurrent_response_rx) = mpsc::channel::<UiRequestResponse>(256);
        let bus = Bus::default();
        let control_rx = bus.control().subscribe();
        let lifecycle_rx = bus.lifecycle().subscribe();
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
            control_rx,
            lifecycle_rx,
            // The absent-source path production takes when no A2A intake is
            // configured: the loop then polls a channel that never closes, so
            // no test exits through a cancel arm it did not drive. A test that
            // drives a cancel passes its own live channel instead.
            task_cancel_rx: None,
            bus,
            ctx_state: CtxState {
                worker_active: false,
                catalog: PickerCatalog::default(),
                pending: None,
            },
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
        catalog: &mut state.catalog,
        span: Span::test_data(),
        stream: bus.stream(),
        control: bus.control(),
        cancel: bus.cancel(),
        lifecycle: bus.lifecycle(),
        narration: bus.narration(),
        facts: bus.facts(),
    }
}

pub(super) fn recv_command(
    worker_rx: &mut Option<mpsc::Receiver<WorkerCommand>>,
) -> Option<WorkerCommand> {
    worker_rx.as_mut()?.try_recv().ok()
}

/// Build the `SourceChannels` view `run_orchestrator_loop` consumes from the
/// channels the test owns beside the loop. Each argument is one disjoint
/// borrow, so the test keeps the senders it drives and the receivers it reads.
pub(super) fn make_sources<'a>(
    worker_result_rx: &'a mut mpsc::Receiver<TurnResult>,
    blocking_response_rx: &'a mut mpsc::Receiver<UiRequestResponse>,
    concurrent_response_rx: &'a mut mpsc::Receiver<UiRequestResponse>,
    control_rx: &'a mut ControlRx,
    lifecycle_rx: &'a mut LifecycleRx,
    task_cancel_rx: &'a mut Option<mpsc::UnboundedReceiver<String>>,
) -> SourceChannels<'a> {
    SourceChannels {
        worker_result_rx,
        blocking_response_rx,
        concurrent_response_rx,
        control_rx,
        lifecycle_rx,
        task_cancel_rx: task_cancel_rx.take(),
        external_task_rx: None,
        external_completion_rx: None,
        external_task_store: None,
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

/// Drain the worker channel and return the task attribution of the first
/// `ExecuteTurn` command, or fail with `what`.
///
/// The turn-start event is published by the worker router, not by the loop, so
/// a loop-level test observes the attribution on the dispatched command.
pub(super) fn recv_execute_turn_task_id(
    worker_rx: &mut Option<mpsc::Receiver<WorkerCommand>>,
    what: &str,
) -> Result<Option<String>> {
    let rx = worker_rx.as_mut().ok_or("worker_rx present")?;
    loop {
        match rx.try_recv() {
            Ok(WorkerCommand::ExecuteTurn { task_id, .. }) => return Ok(task_id),
            Ok(_) => continue,
            Err(_) => return Err(format!("{what}: ExecuteTurn should be dispatched").into()),
        }
    }
}

/// A store that records every rejection and answers with the configured
/// outcome. The loop rejects through it, so the recorded pairs ARE the
/// rejection the caller observes.
#[derive(Default)]
pub(super) struct MockExternalStore {
    rejections: Mutex<Vec<(String, String)>>,
    fails: bool,
}

impl MockExternalStore {
    /// A store whose `reject_task` always fails.
    pub(super) fn failing() -> Self {
        Self {
            fails: true,
            ..Self::default()
        }
    }

    /// The `(task_id, reason)` pairs the loop rejected, in order.
    pub(super) fn rejections(&self) -> Vec<(String, String)> {
        self.rejections
            .lock()
            .map(|recorded| recorded.clone())
            .unwrap_or_default()
    }
}

impl ExternalTaskStore for MockExternalStore {
    fn reject_task(&self, task_id: &str, reason: &str) -> core::result::Result<(), String> {
        self.rejections
            .lock()
            .map_err(|e| e.to_string())?
            .push((task_id.to_string(), reason.to_string()));
        if self.fails {
            return Err("store unavailable".to_string());
        }
        Ok(())
    }
}

/// Build an `ExternalTask` with a resolved context id and a ready-to-submit
/// prompt.
pub(super) fn external_task(task_id: &str, context_id: &str, prompt: &str) -> ExternalTask {
    ExternalTask {
        task_id: task_id.to_string(),
        context_id: context_id.to_string(),
        prompt: prompt.to_string(),
    }
}

/// Build an `ExternalCompletion` carrying the remote task the notification
/// reports and the prompt the boundary formatted.
pub(super) fn external_completion(task_id: &str, prompt: &str) -> ExternalCompletion {
    ExternalCompletion {
        task_id: task_id.to_string(),
        prompt: prompt.to_string(),
    }
}

/// Poll the store until it records a rejection of `task_id`, or fail after 5
/// seconds.
pub(super) async fn wait_for_rejection(
    store: &MockExternalStore,
    task_id: &str,
) -> Result<(String, String)> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let Some(rejection) = store
            .rejections()
            .into_iter()
            .find(|(rejected_id, _)| rejected_id == task_id)
        {
            return Ok(rejection);
        }
        if std::time::Instant::now() >= deadline {
            return Err(format!("external task {task_id} should be rejected").into());
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
}
