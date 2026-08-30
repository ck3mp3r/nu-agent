use nu_agent_a2a::{A2aCompletionEvent, IncomingTask, Message, Part, Role, TaskState};
use nu_protocol::{LabeledError, Span, Value};
use tokio::sync::mpsc;

use crate::bus::{Bus, CompactionEvent, CompactionRx, ExternalEvent, ExternalRx, create_bus};
use crate::conversation::runtime::PendingPermissions;
use crate::orchestrator::stages::{
    OrchestrationContext, PermissionHandler, SessionHandler, SlashHandler, UiRequestHandler,
};
use crate::orchestrator::turn_outcome::TurnOutcome;
use crate::orchestrator::{
    OrchestratorEvent, SourceChannels, Stages, UiRequest, UiRequestResponse, WorkerCommand,
    run_orchestrator_loop,
};
use crate::protocol::event::PermissionDecisionSubmission;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

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

/// Take an owned mpsc receiver out of a `&mut` slot, leaving a placeholder.
fn take_rx<T>(rx: &mut mpsc::Receiver<T>) -> mpsc::Receiver<T> {
    let (_, placeholder) = mpsc::channel::<T>(256);
    std::mem::replace(rx, placeholder)
}

/// Build the `SourceChannels` for `run_orchestrator_loop` from the harness
/// parts. The owned mpsc receivers are taken out of the harness; the broadcast
/// receivers are passed by mutable reference.
fn make_sources<'a>(
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
        worker_result_tx: _,
        worker_result_rx,
        blocking_tx,
        blocking_response_rx,
        concurrent_tx,
        concurrent_response_rx,
        external_rx,
        compaction_rx,
        task_cancel_rx,
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
        make_sources(
            worker_result_rx,
            blocking_response_rx,
            concurrent_response_rx,
            external_rx,
            compaction_rx,
            task_cancel_rx,
        ),
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
        worker_result_tx: _,
        worker_result_rx,
        blocking_tx,
        blocking_response_rx,
        concurrent_tx,
        concurrent_response_rx,
        external_rx,
        compaction_rx,
        task_cancel_rx,
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
        make_sources(
            worker_result_rx,
            blocking_response_rx,
            concurrent_response_rx,
            external_rx,
            compaction_rx,
            task_cancel_rx,
        ),
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
async fn run_compaction_dispatches_when_worker_idle() -> Result<()> {
    let mut h = Harness::new();
    let HarnessParts {
        event_tx: _,
        event_rx,
        worker_tx,
        worker_rx,
        worker_result_tx: _,
        worker_result_rx,
        blocking_tx,
        blocking_response_rx,
        concurrent_tx,
        concurrent_response_rx,
        external_rx,
        compaction_rx,
        task_cancel_rx,
        bus,
        state,
    } = h.parts();
    let mut slash = MockSlash::new();
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::new();
    let mut session = MockSession::new();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);

    bus.compaction()
        .send(CompactionEvent::Requested {
            source: "auto".to_string(),
        })
        .await
        .unwrap();

    let result = run_orchestrator_loop(
        take_event_rx(event_rx),
        make_sources(
            worker_result_rx,
            blocking_response_rx,
            concurrent_response_rx,
            external_rx,
            compaction_rx,
            task_cancel_rx,
        ),
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
    let cmd = recv_command(worker_rx).ok_or("RunCompaction should be dispatched")?;
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
    Ok(())
}

#[tokio::test]
async fn run_compaction_queued_when_worker_busy_then_dispatched_on_idle() -> Result<()> {
    let mut h = Harness::new();
    let HarnessParts {
        event_tx: _,
        event_rx,
        worker_tx,
        worker_rx,
        worker_result_tx,
        worker_result_rx,
        blocking_tx,
        blocking_response_rx,
        concurrent_tx,
        concurrent_response_rx,
        external_rx,
        compaction_rx,
        task_cancel_rx,
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

    bus.compaction()
        .send(CompactionEvent::Requested {
            source: "auto".to_string(),
        })
        .await
        .unwrap();
    // Worker finishes; the queued compaction must be dispatched.
    worker_result_tx
        .send(TurnOutcome::Success(Value::nothing(Span::test_data())))
        .await
        .unwrap();

    let result = run_orchestrator_loop(
        take_event_rx(event_rx),
        make_sources(
            worker_result_rx,
            blocking_response_rx,
            concurrent_response_rx,
            external_rx,
            compaction_rx,
            task_cancel_rx,
        ),
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
    let cmd = recv_command(worker_rx).ok_or("queued RunCompaction should be dispatched")?;
    match cmd {
        WorkerCommand::RunCompaction { source } => {
            assert_eq!(source, "auto");
        }
        _ => panic!("expected RunCompaction"),
    }
    Ok(())
}

#[tokio::test]
async fn run_compaction_queued_replaces_previous_when_busy() -> Result<()> {
    let mut h = Harness::new();
    let HarnessParts {
        event_tx: _,
        event_rx,
        worker_tx,
        worker_rx,
        worker_result_tx: _,
        worker_result_rx,
        blocking_tx,
        blocking_response_rx,
        concurrent_tx,
        concurrent_response_rx,
        external_rx,
        compaction_rx,
        task_cancel_rx,
        bus,
        state,
    } = h.parts();
    let mut slash = MockSlash::new();
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::new();
    let mut session = MockSession::new();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);

    // First request dispatches immediately and marks a compaction active.
    bus.compaction()
        .send(CompactionEvent::Requested {
            source: "auto".to_string(),
        })
        .await
        .unwrap();
    // Second request queued while a compaction is active.
    bus.compaction()
        .send(CompactionEvent::Requested {
            source: "slash".to_string(),
        })
        .await
        .unwrap();
    // Third request replaces the queued one.
    bus.compaction()
        .send(CompactionEvent::Requested {
            source: "manual".to_string(),
        })
        .await
        .unwrap();
    // The active compaction completes; the newest queued request is dispatched.
    bus.compaction()
        .send(CompactionEvent::Completed {
            source: "auto".to_string(),
            summary_preview: String::new(),
            summary_body: String::new(),
        })
        .await
        .unwrap();
    // The dispatched queued compaction completes too, so the loop can exit.
    bus.compaction()
        .send(CompactionEvent::Completed {
            source: "manual".to_string(),
            summary_preview: String::new(),
            summary_body: String::new(),
        })
        .await
        .unwrap();

    let result = run_orchestrator_loop(
        take_event_rx(event_rx),
        make_sources(
            worker_result_rx,
            blocking_response_rx,
            concurrent_response_rx,
            external_rx,
            compaction_rx,
            task_cancel_rx,
        ),
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
    let cmd = recv_command(worker_rx).ok_or("first RunCompaction should be dispatched")?;
    match cmd {
        WorkerCommand::RunCompaction { source } => assert_eq!(source, "auto"),
        _ => panic!("expected RunCompaction"),
    }
    let cmd = recv_command(worker_rx).ok_or("queued RunCompaction should be dispatched")?;
    match cmd {
        WorkerCommand::RunCompaction { source } => {
            assert_eq!(source, "manual", "newest queued request must win");
        }
        _ => panic!("expected RunCompaction"),
    }
    Ok(())
}

#[tokio::test]
async fn compaction_requested_while_active_is_queued() -> Result<()> {
    let mut h = Harness::new();
    let HarnessParts {
        event_tx: _,
        event_rx,
        worker_tx,
        worker_rx,
        worker_result_tx: _,
        worker_result_rx,
        blocking_tx,
        blocking_response_rx,
        concurrent_tx,
        concurrent_response_rx,
        external_rx,
        compaction_rx,
        task_cancel_rx,
        bus,
        state,
    } = h.parts();
    let mut slash = MockSlash::new();
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::new();
    let mut session = MockSession::new();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);

    // First request dispatches immediately (worker idle, no compaction active).
    bus.compaction()
        .send(CompactionEvent::Requested {
            source: "auto".to_string(),
        })
        .await
        .unwrap();
    // Second request arrives while a compaction is active — it must be queued,
    // not dispatched.
    bus.compaction()
        .send(CompactionEvent::Requested {
            source: "slash".to_string(),
        })
        .await
        .unwrap();
    // The active compaction completes; the queued request is dispatched.
    bus.compaction()
        .send(CompactionEvent::Completed {
            source: "auto".to_string(),
            summary_preview: String::new(),
            summary_body: String::new(),
        })
        .await
        .unwrap();
    // The dispatched queued compaction completes too, so the loop can exit.
    bus.compaction()
        .send(CompactionEvent::Completed {
            source: "slash".to_string(),
            summary_preview: String::new(),
            summary_body: String::new(),
        })
        .await
        .unwrap();

    let result = run_orchestrator_loop(
        take_event_rx(event_rx),
        make_sources(
            worker_result_rx,
            blocking_response_rx,
            concurrent_response_rx,
            external_rx,
            compaction_rx,
            task_cancel_rx,
        ),
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
    // First command: the initial request.
    let cmd = recv_command(worker_rx).ok_or("first RunCompaction should be dispatched")?;
    match cmd {
        WorkerCommand::RunCompaction { source } => assert_eq!(source, "auto"),
        _ => panic!("expected RunCompaction"),
    }
    // Second command: the queued request dispatched after completion.
    let cmd = recv_command(worker_rx).ok_or("queued RunCompaction should be dispatched")?;
    match cmd {
        WorkerCommand::RunCompaction { source } => assert_eq!(source, "slash"),
        _ => panic!("expected RunCompaction"),
    }
    // No third command should be dispatched.
    assert!(
        recv_command(worker_rx).is_none(),
        "no further RunCompaction should be dispatched"
    );
    Ok(())
}

#[tokio::test]
async fn quit_waits_while_compaction_active() -> Result<()> {
    let h = Harness::new();
    let Harness {
        event_tx,
        event_rx,
        worker_tx,
        mut worker_rx,
        worker_result_tx: _,
        mut worker_result_rx,
        blocking_tx,
        mut blocking_response_rx,
        concurrent_tx,
        mut concurrent_response_rx,
        mut external_rx,
        mut compaction_rx,
        task_cancel_rx: _,
        bus,
        mut ctx_state,
    } = h;

    // Run the loop concurrently. Pass `None` for the task-cancel source so the
    // loop does not break on a closed cancel channel; it can only exit via Quit.
    let bus_driver = bus.clone();
    let loop_task = tokio::spawn(async move {
        let mut slash = MockSlash::new();
        let mut permission = MockPermission::new();
        let mut ui_request = MockUiRequest::new();
        let mut session = MockSession::new();
        let mut ctx = make_ctx(
            &worker_tx,
            &blocking_tx,
            &concurrent_tx,
            &bus,
            &mut ctx_state,
        );
        let mut task_cancel_rx: Option<mpsc::UnboundedReceiver<String>> = None;
        run_orchestrator_loop(
            event_rx,
            make_sources(
                &mut worker_result_rx,
                &mut blocking_response_rx,
                &mut concurrent_response_rx,
                &mut external_rx,
                &mut compaction_rx,
                &mut task_cancel_rx,
            ),
            Stages {
                slash: &mut slash,
                permission: &mut permission,
                ui_request: &mut ui_request,
                session: &mut session,
            },
            &mut ctx,
        )
        .await
    });

    // Start a compaction so it is active when Quit arrives.
    bus_driver
        .compaction()
        .send(CompactionEvent::Requested {
            source: "auto".to_string(),
        })
        .await
        .unwrap();
    // Wait until the compaction command is dispatched (compaction is active).
    let cmd = worker_rx.as_mut().ok_or("worker_rx present")?.recv();
    let cmd = tokio::time::timeout(std::time::Duration::from_secs(5), cmd)
        .await
        .map_err(|_| "RunCompaction should be dispatched")?
        .ok_or("worker channel should not close")?;
    match cmd {
        WorkerCommand::RunCompaction { source } => assert_eq!(source, "auto"),
        _ => panic!("expected RunCompaction"),
    }

    // Request quit while the compaction is still active.
    event_tx.send(OrchestratorEvent::Quit).await.unwrap();
    // Give the loop a chance to process Quit; it must NOT exit yet.
    tokio::task::yield_now().await;
    assert!(
        !loop_task.is_finished(),
        "loop must not exit while a compaction is active"
    );

    // The compaction completes; the loop may now exit.
    bus_driver
        .compaction()
        .send(CompactionEvent::Completed {
            source: "auto".to_string(),
            summary_preview: String::new(),
            summary_body: String::new(),
        })
        .await
        .unwrap();

    let result = loop_task
        .await
        .map_err(|e| format!("loop task should not panic: {e}"))?;
    assert!(result.is_ok());
    Ok(())
}

#[tokio::test]
async fn quit_exits_after_compaction_completes() -> Result<()> {
    let h = Harness::new();
    let Harness {
        event_tx,
        event_rx,
        worker_tx,
        mut worker_rx,
        worker_result_tx: _,
        mut worker_result_rx,
        blocking_tx,
        mut blocking_response_rx,
        concurrent_tx,
        mut concurrent_response_rx,
        mut external_rx,
        mut compaction_rx,
        task_cancel_rx: _,
        bus,
        mut ctx_state,
    } = h;

    let bus_driver = bus.clone();
    let loop_task = tokio::spawn(async move {
        let mut slash = MockSlash::new();
        let mut permission = MockPermission::new();
        let mut ui_request = MockUiRequest::new();
        let mut session = MockSession::new();
        let mut ctx = make_ctx(
            &worker_tx,
            &blocking_tx,
            &concurrent_tx,
            &bus,
            &mut ctx_state,
        );
        let mut task_cancel_rx: Option<mpsc::UnboundedReceiver<String>> = None;
        run_orchestrator_loop(
            event_rx,
            make_sources(
                &mut worker_result_rx,
                &mut blocking_response_rx,
                &mut concurrent_response_rx,
                &mut external_rx,
                &mut compaction_rx,
                &mut task_cancel_rx,
            ),
            Stages {
                slash: &mut slash,
                permission: &mut permission,
                ui_request: &mut ui_request,
                session: &mut session,
            },
            &mut ctx,
        )
        .await
    });

    // Start a compaction so it is active when Quit arrives.
    bus_driver
        .compaction()
        .send(CompactionEvent::Requested {
            source: "auto".to_string(),
        })
        .await
        .unwrap();
    // Wait until the compaction command is dispatched (compaction is active).
    let cmd = worker_rx.as_mut().ok_or("worker_rx present")?.recv();
    let cmd = tokio::time::timeout(std::time::Duration::from_secs(5), cmd)
        .await
        .map_err(|_| "RunCompaction should be dispatched")?
        .ok_or("worker channel should not close")?;
    match cmd {
        WorkerCommand::RunCompaction { source } => assert_eq!(source, "auto"),
        _ => panic!("expected RunCompaction"),
    }

    // Request quit while the compaction is active, then complete the compaction.
    event_tx.send(OrchestratorEvent::Quit).await.unwrap();
    bus_driver
        .compaction()
        .send(CompactionEvent::Completed {
            source: "auto".to_string(),
            summary_preview: String::new(),
            summary_body: String::new(),
        })
        .await
        .unwrap();

    let result = loop_task
        .await
        .map_err(|e| format!("loop task should not panic: {e}"))?;
    assert!(result.is_ok());
    Ok(())
}

#[tokio::test]
async fn worker_result_drains_queued_and_checks_compaction() {
    let mut h = Harness::new();
    let HarnessParts {
        event_tx: _,
        event_rx,
        worker_tx,
        worker_rx: _,
        worker_result_tx,
        worker_result_rx,
        blocking_tx,
        blocking_response_rx,
        concurrent_tx,
        concurrent_response_rx,
        external_rx,
        compaction_rx,
        task_cancel_rx,
        bus,
        state,
    } = h.parts();
    let mut slash = MockSlash::new();
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::new();
    let mut session = MockSession::new();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);

    worker_result_tx
        .send(TurnOutcome::Success(Value::nothing(Span::test_data())))
        .await
        .unwrap();

    let result = run_orchestrator_loop(
        take_event_rx(event_rx),
        make_sources(
            worker_result_rx,
            blocking_response_rx,
            concurrent_response_rx,
            external_rx,
            compaction_rx,
            task_cancel_rx,
        ),
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
        worker_result_tx: _,
        worker_result_rx,
        blocking_tx,
        blocking_response_rx,
        concurrent_tx,
        concurrent_response_rx,
        external_rx,
        compaction_rx,
        task_cancel_rx,
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
        make_sources(
            worker_result_rx,
            blocking_response_rx,
            concurrent_response_rx,
            external_rx,
            compaction_rx,
            task_cancel_rx,
        ),
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
        worker_result_tx: _,
        worker_result_rx,
        blocking_tx,
        blocking_response_rx,
        concurrent_tx,
        concurrent_response_rx,
        external_rx,
        compaction_rx,
        task_cancel_rx,
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
        make_sources(
            worker_result_rx,
            blocking_response_rx,
            concurrent_response_rx,
            external_rx,
            compaction_rx,
            task_cancel_rx,
        ),
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
async fn external_prompt_dispatches_turn_when_idle() -> Result<()> {
    let mut h = Harness::new();
    let HarnessParts {
        event_tx: _,
        event_rx,
        worker_tx,
        worker_rx,
        worker_result_tx,
        worker_result_rx,
        blocking_tx,
        blocking_response_rx,
        concurrent_tx,
        concurrent_response_rx,
        external_rx,
        compaction_rx,
        task_cancel_rx,
        bus,
        state,
    } = h.parts();
    let mut slash = MockSlash::new();
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::new();
    let mut session = MockSession::new();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);

    bus.external()
        .send(ExternalEvent::PromptReceived {
            prompt: "external task".to_string(),
            task_id: "task-1".to_string(),
        })
        .await
        .unwrap();
    worker_result_tx
        .send(TurnOutcome::Success(Value::nothing(Span::test_data())))
        .await
        .unwrap();

    let result = run_orchestrator_loop(
        take_event_rx(event_rx),
        make_sources(
            worker_result_rx,
            blocking_response_rx,
            concurrent_response_rx,
            external_rx,
            compaction_rx,
            task_cancel_rx,
        ),
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
    let cmd = recv_command(worker_rx).ok_or("ExecuteTurn should be dispatched")?;
    match cmd {
        WorkerCommand::ExecuteTurn { prompt, .. } => {
            assert_eq!(prompt, "external task");
        }
        _ => panic!("expected ExecuteTurn"),
    }
    Ok(())
}

#[tokio::test]
async fn fatal_error_returns_err() -> Result<()> {
    let mut h = Harness::new();
    let HarnessParts {
        event_tx,
        event_rx,
        worker_tx,
        worker_rx: _,
        worker_result_tx: _,
        worker_result_rx,
        blocking_tx,
        blocking_response_rx,
        concurrent_tx,
        concurrent_response_rx,
        external_rx,
        compaction_rx,
        task_cancel_rx,
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
        make_sources(
            worker_result_rx,
            blocking_response_rx,
            concurrent_response_rx,
            external_rx,
            compaction_rx,
            task_cancel_rx,
        ),
        Stages {
            slash: &mut slash,
            permission: &mut permission,
            ui_request: &mut ui_request,
            session: &mut session,
        },
        &mut ctx,
    )
    .await;

    let Err(err) = result else {
        return Err("run_orchestrator_loop should fail with fatal error".into());
    };
    assert_eq!(err.msg, "fatal");
    Ok(())
}

#[tokio::test]
async fn a2a_task_rx_dispatches_turn() -> Result<()> {
    let h = Harness::new();
    let Harness {
        event_tx,
        event_rx,
        worker_tx,
        mut worker_rx,
        worker_result_tx,
        mut worker_result_rx,
        blocking_tx,
        mut blocking_response_rx,
        concurrent_tx,
        mut concurrent_response_rx,
        mut external_rx,
        mut compaction_rx,
        task_cancel_rx: _,
        bus,
        mut ctx_state,
    } = h;

    let (a2a_task_tx, a2a_task_rx) = mpsc::channel::<IncomingTask>(16);

    let loop_task = tokio::spawn(async move {
        let mut slash = MockSlash::new();
        let mut permission = MockPermission::new();
        let mut ui_request = MockUiRequest::new();
        let mut session = MockSession::new();
        let mut ctx = make_ctx(
            &worker_tx,
            &blocking_tx,
            &concurrent_tx,
            &bus,
            &mut ctx_state,
        );
        let mut task_cancel_rx: Option<mpsc::UnboundedReceiver<String>> = None;
        let mut sources = make_sources(
            &mut worker_result_rx,
            &mut blocking_response_rx,
            &mut concurrent_response_rx,
            &mut external_rx,
            &mut compaction_rx,
            &mut task_cancel_rx,
        );
        sources.a2a_task_rx = Some(a2a_task_rx);
        run_orchestrator_loop(
            event_rx,
            sources,
            Stages {
                slash: &mut slash,
                permission: &mut permission,
                ui_request: &mut ui_request,
                session: &mut session,
            },
            &mut ctx,
        )
        .await
    });

    a2a_task_tx
        .send(IncomingTask {
            task_id: "task-1".to_string(),
            message: Message {
                role: Role::User,
                parts: vec![Part::Text {
                    text: "do work".to_string(),
                }],
                message_id: "msg-1".to_string(),
                extensions: None,
                metadata: None,
            },
            sender_url: "http://a.local".to_string(),
            session_id: None,
            context_id: None,
            parent_task_id: None,
        })
        .await
        .unwrap();

    let cmd = worker_rx.as_mut().ok_or("worker_rx present")?.recv();
    let cmd = tokio::time::timeout(std::time::Duration::from_secs(5), cmd)
        .await
        .map_err(|_| "ExecuteTurn should be dispatched")?
        .ok_or("worker channel should not close")?;
    match cmd {
        WorkerCommand::ExecuteTurn { prompt, .. } => {
            assert_eq!(
                prompt,
                "[A2A Task task-1 from http://a.local]: do work\n\nProcess this request and respond with your answer. Your response will be automatically delivered as the task result."
            );
        }
        _ => panic!("expected ExecuteTurn"),
    }

    // Make the worker idle, then quit so the loop exits cleanly.
    worker_result_tx
        .send(TurnOutcome::Success(Value::nothing(Span::test_data())))
        .await
        .unwrap();
    event_tx.send(OrchestratorEvent::Quit).await.unwrap();

    let result = loop_task
        .await
        .map_err(|e| format!("loop task should not panic: {e}"))?;
    assert!(result.is_ok());
    Ok(())
}

#[tokio::test]
async fn a2a_completion_rx_dispatches_turn() -> Result<()> {
    let h = Harness::new();
    let Harness {
        event_tx,
        event_rx,
        worker_tx,
        mut worker_rx,
        worker_result_tx,
        mut worker_result_rx,
        blocking_tx,
        mut blocking_response_rx,
        concurrent_tx,
        mut concurrent_response_rx,
        mut external_rx,
        mut compaction_rx,
        task_cancel_rx: _,
        bus,
        mut ctx_state,
    } = h;

    let (a2a_completion_tx, a2a_completion_rx) = mpsc::channel::<A2aCompletionEvent>(16);

    let loop_task = tokio::spawn(async move {
        let mut slash = MockSlash::new();
        let mut permission = MockPermission::new();
        let mut ui_request = MockUiRequest::new();
        let mut session = MockSession::new();
        let mut ctx = make_ctx(
            &worker_tx,
            &blocking_tx,
            &concurrent_tx,
            &bus,
            &mut ctx_state,
        );
        let mut task_cancel_rx: Option<mpsc::UnboundedReceiver<String>> = None;
        let mut sources = make_sources(
            &mut worker_result_rx,
            &mut blocking_response_rx,
            &mut concurrent_response_rx,
            &mut external_rx,
            &mut compaction_rx,
            &mut task_cancel_rx,
        );
        sources.a2a_completion_rx = Some(a2a_completion_rx);
        run_orchestrator_loop(
            event_rx,
            sources,
            Stages {
                slash: &mut slash,
                permission: &mut permission,
                ui_request: &mut ui_request,
                session: &mut session,
            },
            &mut ctx,
        )
        .await
    });

    a2a_completion_tx
        .send(A2aCompletionEvent {
            task_id: "task-2".to_string(),
            agent_name: "agent-b".to_string(),
            result: "all done".to_string(),
            status: TaskState::Completed,
        })
        .await
        .unwrap();

    let cmd = worker_rx.as_mut().ok_or("worker_rx present")?.recv();
    let cmd = tokio::time::timeout(std::time::Duration::from_secs(5), cmd)
        .await
        .map_err(|_| "ExecuteTurn should be dispatched")?
        .ok_or("worker channel should not close")?;
    match cmd {
        WorkerCommand::ExecuteTurn { prompt, .. } => {
            assert_eq!(
                prompt,
                "[A2A Task task-2 completed by agent-b]: all done\n\nStatus: TASK_STATE_COMPLETED."
            );
        }
        _ => panic!("expected ExecuteTurn"),
    }

    // Make the worker idle, then quit so the loop exits cleanly.
    worker_result_tx
        .send(TurnOutcome::Success(Value::nothing(Span::test_data())))
        .await
        .unwrap();
    event_tx.send(OrchestratorEvent::Quit).await.unwrap();

    let result = loop_task
        .await
        .map_err(|e| format!("loop task should not panic: {e}"))?;
    assert!(result.is_ok());
    Ok(())
}
