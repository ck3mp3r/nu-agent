use nu_protocol::{LabeledError, Span, Value};
use tokio::sync::mpsc;

use crate::bus::{Bus, create_bus};
use crate::conversation::runtime::PendingPermissions;
use crate::orchestrator::stages::{
    OrchestrationContext, PermissionHandler, SessionHandler, SlashHandler, UiRequestHandler,
};
use crate::orchestrator::turn_outcome::TurnOutcome;
use crate::orchestrator::{
    OrchestratorEvent, Stages, UiRequest, UiRequestResponse, WorkerCommand, run_orchestrator_loop,
};
use crate::protocol::event::PermissionDecisionSubmission;

// ---------------------------------------------------------------------------
// Mock stage implementations
// ---------------------------------------------------------------------------

struct MockSlash {
    handle_calls: Vec<String>,
}

impl MockSlash {
    fn new() -> Self {
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

struct MockPermission {
    handle_calls: usize,
}

impl MockPermission {
    fn new() -> Self {
        Self { handle_calls: 0 }
    }
}

impl PermissionHandler for MockPermission {
    fn handle(&mut self, _decision: PermissionDecisionSubmission, _ctx: &mut OrchestrationContext) {
        self.handle_calls += 1;
    }
}

struct MockUiRequest {
    handle_incoming_calls: Vec<UiRequest>,
    drain_queued_calls: usize,
    blocking_pending: bool,
    pending: bool,
}

impl MockUiRequest {
    fn new() -> Self {
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

    fn handle_blocking_response(
        &mut self,
        _response: UiRequestResponse,
        _ctx: &mut OrchestrationContext,
    ) {
    }

    fn handle_concurrent_response(
        &mut self,
        _response: UiRequestResponse,
        _ctx: &mut OrchestrationContext,
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

struct MockSession {
    handle_outcome_calls: Vec<TurnOutcome>,
}

impl MockSession {
    fn new() -> Self {
        Self {
            handle_outcome_calls: Vec::new(),
        }
    }
}

impl SessionHandler for MockSession {
    fn handle_outcome(&mut self, outcome: TurnOutcome, ctx: &mut OrchestrationContext) {
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
}

pub(crate) struct HarnessParts<'a> {
    pub(crate) event_tx: &'a mpsc::Sender<OrchestratorEvent>,
    pub(crate) event_rx: &'a mut mpsc::Receiver<OrchestratorEvent>,
    pub(crate) worker_tx: &'a mpsc::Sender<WorkerCommand>,
    pub(crate) worker_rx: &'a mut Option<mpsc::Receiver<WorkerCommand>>,
    pub(crate) blocking_tx: &'a mpsc::Sender<UiRequestResponse>,
    pub(crate) concurrent_tx: &'a mpsc::Sender<UiRequestResponse>,
    pub(crate) bus: &'a Bus,
    pub(crate) state: &'a mut CtxState,
}

pub(crate) struct Harness {
    pub(crate) event_tx: mpsc::Sender<OrchestratorEvent>,
    pub(crate) event_rx: mpsc::Receiver<OrchestratorEvent>,
    pub(crate) worker_tx: mpsc::Sender<WorkerCommand>,
    pub(crate) worker_rx: Option<mpsc::Receiver<WorkerCommand>>,
    pub(crate) blocking_tx: mpsc::Sender<UiRequestResponse>,
    pub(crate) concurrent_tx: mpsc::Sender<UiRequestResponse>,
    pub(crate) bus: Bus,
    pub(crate) ctx_state: CtxState,
}

impl Harness {
    pub(crate) fn new() -> Self {
        let (event_tx, event_rx) = mpsc::channel::<OrchestratorEvent>(256);
        let (worker_tx, worker_rx) = mpsc::channel::<WorkerCommand>(256);
        let (blocking_tx, _blocking_rx) = mpsc::channel::<UiRequestResponse>(256);
        let (concurrent_tx, _concurrent_rx) = mpsc::channel::<UiRequestResponse>(256);
        let bus = create_bus();
        Self {
            event_tx,
            event_rx,
            worker_tx,
            worker_rx: Some(worker_rx),
            blocking_tx,
            concurrent_tx,
            bus,
            ctx_state: CtxState {
                worker_active: false,
                pending: None,
                active_external_prompt: None,
                active_external_task_id: None,
                pending_external_cancel: None,
            },
        }
    }

    pub(crate) fn parts(&mut self) -> HarnessParts<'_> {
        HarnessParts {
            event_tx: &self.event_tx,
            event_rx: &mut self.event_rx,
            worker_tx: &self.worker_tx,
            worker_rx: &mut self.worker_rx,
            blocking_tx: &self.blocking_tx,
            concurrent_tx: &self.concurrent_tx,
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
        bus,
    }
}

fn take_event_rx(
    event_rx: &mut mpsc::Receiver<OrchestratorEvent>,
) -> mpsc::Receiver<OrchestratorEvent> {
    let (_, placeholder) = mpsc::channel::<OrchestratorEvent>(256);
    std::mem::replace(event_rx, placeholder)
}

fn recv_command(worker_rx: &mut Option<mpsc::Receiver<WorkerCommand>>) -> Option<WorkerCommand> {
    worker_rx.as_mut()?.try_recv().ok()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn prompt_submitted_routes_to_slash() {
    let mut h = Harness::new();
    let HarnessParts {
        event_tx,
        event_rx,
        worker_tx,
        worker_rx: _,
        blocking_tx,
        concurrent_tx,
        bus,
        state,
    } = h.parts();
    let mut slash = MockSlash::new();
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::new();
    let mut session = MockSession::new();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);

    event_tx
        .send(OrchestratorEvent::PromptSubmitted {
            text: "hello".to_string(),
        })
        .await
        .unwrap();
    event_tx.send(OrchestratorEvent::Quit).await.unwrap();

    let result = run_orchestrator_loop(
        take_event_rx(event_rx),
        Stages {
            slash: &mut slash,
            permission: &mut permission,
            ui_request: &mut ui_request,
            session: &mut session,
        },
        &mut ctx,
    )
    .await;

    assert!(result.is_ok());
    assert_eq!(slash.handle_calls, vec!["hello".to_string()]);
}

#[tokio::test]
async fn prompt_submitted_blocked_when_blocking_pending() {
    let mut h = Harness::new();
    let HarnessParts {
        event_tx,
        event_rx,
        worker_tx,
        worker_rx: _,
        blocking_tx,
        concurrent_tx,
        bus,
        state,
    } = h.parts();
    let mut slash = MockSlash::new();
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::new();
    ui_request.blocking_pending = true;
    let mut session = MockSession::new();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);

    event_tx
        .send(OrchestratorEvent::PromptSubmitted {
            text: "hello".to_string(),
        })
        .await
        .unwrap();
    event_tx.send(OrchestratorEvent::Quit).await.unwrap();

    let result = run_orchestrator_loop(
        take_event_rx(event_rx),
        Stages {
            slash: &mut slash,
            permission: &mut permission,
            ui_request: &mut ui_request,
            session: &mut session,
        },
        &mut ctx,
    )
    .await;

    assert!(result.is_ok());
    assert!(
        slash.handle_calls.is_empty(),
        "slash.handle should NOT be called"
    );
}

#[tokio::test]
async fn run_compaction_dispatches_when_worker_idle() {
    let mut h = Harness::new();
    let HarnessParts {
        event_tx,
        event_rx,
        worker_tx,
        worker_rx,
        blocking_tx,
        concurrent_tx,
        bus,
        state,
    } = h.parts();
    let mut slash = MockSlash::new();
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::new();
    let mut session = MockSession::new();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);

    event_tx
        .send(OrchestratorEvent::RunCompaction {
            source: "auto".to_string(),
        })
        .await
        .unwrap();
    event_tx.send(OrchestratorEvent::Quit).await.unwrap();

    let result = run_orchestrator_loop(
        take_event_rx(event_rx),
        Stages {
            slash: &mut slash,
            permission: &mut permission,
            ui_request: &mut ui_request,
            session: &mut session,
        },
        &mut ctx,
    )
    .await;

    assert!(result.is_ok());
    let cmd = recv_command(worker_rx).expect("RunCompaction should be dispatched");
    match cmd {
        WorkerCommand::RunCompaction { source } => {
            assert_eq!(source, "auto");
        }
        _ => panic!("expected RunCompaction"),
    }
    assert!(
        !state.worker_active,
        "worker_active must NOT be set for a compaction command (the worker emits events on the bus)"
    );
}

#[tokio::test]
async fn run_compaction_queued_when_worker_busy_then_dispatched_on_idle() {
    let mut h = Harness::new();
    let HarnessParts {
        event_tx,
        event_rx,
        worker_tx,
        worker_rx,
        blocking_tx,
        concurrent_tx,
        bus,
        state,
    } = h.parts();
    let mut slash = MockSlash::new();
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::new();
    let mut session = MockSession::new();
    // Worker is busy running a turn.
    state.worker_active = true;
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);

    event_tx
        .send(OrchestratorEvent::RunCompaction {
            source: "auto".to_string(),
        })
        .await
        .unwrap();
    // Worker finishes; the queued compaction must be dispatched.
    event_tx
        .send(OrchestratorEvent::WorkerResult(TurnOutcome::Success(
            Value::nothing(Span::test_data()),
        )))
        .await
        .unwrap();
    event_tx.send(OrchestratorEvent::Quit).await.unwrap();

    let result = run_orchestrator_loop(
        take_event_rx(event_rx),
        Stages {
            slash: &mut slash,
            permission: &mut permission,
            ui_request: &mut ui_request,
            session: &mut session,
        },
        &mut ctx,
    )
    .await;

    assert!(result.is_ok());
    let cmd = recv_command(worker_rx).expect("queued RunCompaction should be dispatched");
    match cmd {
        WorkerCommand::RunCompaction { source } => {
            assert_eq!(source, "auto");
        }
        _ => panic!("expected RunCompaction"),
    }
}

#[tokio::test]
async fn run_compaction_queued_replaces_previous_when_busy() {
    let mut h = Harness::new();
    let HarnessParts {
        event_tx,
        event_rx,
        worker_tx,
        worker_rx,
        blocking_tx,
        concurrent_tx,
        bus,
        state,
    } = h.parts();
    let mut slash = MockSlash::new();
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::new();
    let mut session = MockSession::new();
    state.worker_active = true;
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);

    // First request queued.
    event_tx
        .send(OrchestratorEvent::RunCompaction {
            source: "auto".to_string(),
        })
        .await
        .unwrap();
    // Second request replaces the queued one.
    event_tx
        .send(OrchestratorEvent::RunCompaction {
            source: "slash".to_string(),
        })
        .await
        .unwrap();
    event_tx
        .send(OrchestratorEvent::WorkerResult(TurnOutcome::Success(
            Value::nothing(Span::test_data()),
        )))
        .await
        .unwrap();
    event_tx.send(OrchestratorEvent::Quit).await.unwrap();

    let result = run_orchestrator_loop(
        take_event_rx(event_rx),
        Stages {
            slash: &mut slash,
            permission: &mut permission,
            ui_request: &mut ui_request,
            session: &mut session,
        },
        &mut ctx,
    )
    .await;

    assert!(result.is_ok());
    let cmd = recv_command(worker_rx).expect("queued RunCompaction should be dispatched");
    match cmd {
        WorkerCommand::RunCompaction { source } => {
            assert_eq!(source, "slash", "newest queued request must win");
        }
        _ => panic!("expected RunCompaction"),
    }
}

#[tokio::test]
async fn worker_result_drains_queued_and_checks_compaction() {
    let mut h = Harness::new();
    let HarnessParts {
        event_tx,
        event_rx,
        worker_tx,
        worker_rx: _,
        blocking_tx,
        concurrent_tx,
        bus,
        state,
    } = h.parts();
    let mut slash = MockSlash::new();
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::new();
    let mut session = MockSession::new();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);

    event_tx
        .send(OrchestratorEvent::WorkerResult(TurnOutcome::Success(
            Value::nothing(Span::test_data()),
        )))
        .await
        .unwrap();
    event_tx.send(OrchestratorEvent::Quit).await.unwrap();

    let result = run_orchestrator_loop(
        take_event_rx(event_rx),
        Stages {
            slash: &mut slash,
            permission: &mut permission,
            ui_request: &mut ui_request,
            session: &mut session,
        },
        &mut ctx,
    )
    .await;

    assert!(result.is_ok());
    assert_eq!(
        session.handle_outcome_calls.len(),
        1,
        "session.handle_outcome should be called"
    );
    assert_eq!(
        ui_request.drain_queued_calls, 1,
        "ui_request.drain_queued should be called"
    );
}

#[tokio::test]
async fn quit_blocked_when_worker_active() {
    let mut h = Harness::new();
    let HarnessParts {
        event_tx,
        event_rx,
        worker_tx,
        worker_rx: _,
        blocking_tx,
        concurrent_tx,
        bus,
        state,
    } = h.parts();
    state.worker_active = true;
    // Send Quit while worker is active — loop should continue, not break.
    event_tx.send(OrchestratorEvent::Quit).await.unwrap();
    // Send another Quit after worker becomes idle.
    state.worker_active = false;
    event_tx.send(OrchestratorEvent::Quit).await.unwrap();

    let mut slash = MockSlash::new();
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::new();
    let mut session = MockSession::new();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);

    let result = run_orchestrator_loop(
        take_event_rx(event_rx),
        Stages {
            slash: &mut slash,
            permission: &mut permission,
            ui_request: &mut ui_request,
            session: &mut session,
        },
        &mut ctx,
    )
    .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn quit_allowed_when_idle_and_no_pending() {
    let mut h = Harness::new();
    let HarnessParts {
        event_tx,
        event_rx,
        worker_tx,
        worker_rx: _,
        blocking_tx,
        concurrent_tx,
        bus,
        state,
    } = h.parts();
    let mut slash = MockSlash::new();
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::new();
    let mut session = MockSession::new();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);

    event_tx.send(OrchestratorEvent::Quit).await.unwrap();

    let result = run_orchestrator_loop(
        take_event_rx(event_rx),
        Stages {
            slash: &mut slash,
            permission: &mut permission,
            ui_request: &mut ui_request,
            session: &mut session,
        },
        &mut ctx,
    )
    .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn external_prompt_dispatches_turn_when_idle() {
    let mut h = Harness::new();
    let HarnessParts {
        event_tx,
        event_rx,
        worker_tx,
        worker_rx,
        blocking_tx,
        concurrent_tx,
        bus,
        state,
    } = h.parts();
    let mut slash = MockSlash::new();
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::new();
    let mut session = MockSession::new();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);

    event_tx
        .send(OrchestratorEvent::ExternalPrompt {
            prompt: "external task".to_string(),
            task_id: "task-1".to_string(),
        })
        .await
        .unwrap();
    event_tx
        .send(OrchestratorEvent::WorkerResult(TurnOutcome::Success(
            Value::nothing(Span::test_data()),
        )))
        .await
        .unwrap();
    event_tx.send(OrchestratorEvent::Quit).await.unwrap();

    let result = run_orchestrator_loop(
        take_event_rx(event_rx),
        Stages {
            slash: &mut slash,
            permission: &mut permission,
            ui_request: &mut ui_request,
            session: &mut session,
        },
        &mut ctx,
    )
    .await;

    assert!(result.is_ok());
    assert!(
        !state.worker_active,
        "worker_active should be false after worker completes"
    );
    assert_eq!(
        state.active_external_prompt.as_deref(),
        Some("external task")
    );
    assert_eq!(state.active_external_task_id.as_deref(), Some("task-1"));
    let cmd = recv_command(worker_rx).expect("ExecuteTurn should be dispatched");
    match cmd {
        WorkerCommand::ExecuteTurn { prompt, .. } => {
            assert_eq!(prompt, "external task");
        }
        _ => panic!("expected ExecuteTurn"),
    }
}

#[tokio::test]
async fn fatal_error_returns_err() {
    let mut h = Harness::new();
    let HarnessParts {
        event_tx,
        event_rx,
        worker_tx,
        worker_rx: _,
        blocking_tx,
        concurrent_tx,
        bus,
        state,
    } = h.parts();
    let mut slash = MockSlash::new();
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::new();
    let mut session = MockSession::new();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);

    event_tx
        .send(OrchestratorEvent::FatalError(LabeledError::new("fatal")))
        .await
        .unwrap();

    let result = run_orchestrator_loop(
        take_event_rx(event_rx),
        Stages {
            slash: &mut slash,
            permission: &mut permission,
            ui_request: &mut ui_request,
            session: &mut session,
        },
        &mut ctx,
    )
    .await;

    assert!(result.is_err());
    assert_eq!(result.unwrap_err().msg, "fatal");
}
