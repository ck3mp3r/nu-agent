use nu_protocol::Span;
use tokio::sync::mpsc;

use crate::bus::{Bus, SessionEvent, WarningEvent, create_bus};
use crate::conversation::runtime::PendingPermissions;
use crate::orchestrator::stages::ui_request::UiRequestStage;
use crate::orchestrator::stages::{OrchestrationContext, UiRequestHandler};
use crate::orchestrator::{UiRequest, UiRequestResponse, UiStateEvent, WorkerCommand};
use crate::protocol::contracts::{McpUsabilityState, UiMessageSnapshot};
use crate::session::SessionInfo;

/// Mutable per-test state that the `UiRequestStage` reads and writes through
/// `OrchestrationContext`. Kept separate from the channels so `ctx()` can
/// borrow it disjointly from `stage` and the receivers.
struct CtxState {
    worker_active: bool,
    pending: Option<PendingPermissions>,
    active_external_prompt: Option<String>,
    active_external_task_id: Option<String>,
    pending_external_cancel: Option<String>,
}

struct HarnessParts<'a> {
    stage: &'a mut UiRequestStage,
    worker_tx: &'a mpsc::Sender<WorkerCommand>,
    blocking_tx: &'a mpsc::Sender<UiRequestResponse>,
    concurrent_tx: &'a mpsc::Sender<UiRequestResponse>,
    bus: &'a Bus,
    worker_rx: &'a mut Option<mpsc::Receiver<WorkerCommand>>,
    warning_rx: &'a mut tokio::sync::broadcast::Receiver<WarningEvent>,
    ui_state_rx: &'a mut tokio::sync::broadcast::Receiver<UiStateEvent>,
    session_rx: &'a mut tokio::sync::broadcast::Receiver<SessionEvent>,
    ctx_state: &'a mut CtxState,
}

struct Harness {
    stage: UiRequestStage,
    worker_tx: mpsc::Sender<WorkerCommand>,
    worker_rx: Option<mpsc::Receiver<WorkerCommand>>,
    blocking_tx: mpsc::Sender<UiRequestResponse>,
    concurrent_tx: mpsc::Sender<UiRequestResponse>,
    bus: Bus,
    warning_rx: tokio::sync::broadcast::Receiver<WarningEvent>,
    ui_state_rx: tokio::sync::broadcast::Receiver<UiStateEvent>,
    session_rx: tokio::sync::broadcast::Receiver<SessionEvent>,
    ctx_state: CtxState,
}

impl Harness {
    fn new() -> Self {
        let (worker_tx, worker_rx) = mpsc::channel::<WorkerCommand>(256);
        let (blocking_tx, _blocking_rx) = mpsc::channel::<UiRequestResponse>(256);
        let (concurrent_tx, _concurrent_rx) = mpsc::channel::<UiRequestResponse>(256);
        let bus = create_bus();
        let warning_rx = bus.warning().subscribe();
        let ui_state_rx = bus.ui_state().subscribe();
        let session_rx = bus.session().subscribe();
        Self {
            stage: UiRequestStage::new(0),
            worker_tx,
            worker_rx: Some(worker_rx),
            blocking_tx,
            concurrent_tx,
            warning_rx,
            ui_state_rx,
            session_rx,
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

    /// Decompose the harness into fully disjoint borrows so a long-lived
    /// `OrchestrationContext` (built via the free `ctx()` below) does not
    /// conflict with the receivers used by `recv_*`/`take_*`.
    fn parts(&mut self) -> HarnessParts<'_> {
        HarnessParts {
            stage: &mut self.stage,
            worker_tx: &self.worker_tx,
            blocking_tx: &self.blocking_tx,
            concurrent_tx: &self.concurrent_tx,
            bus: &self.bus,
            worker_rx: &mut self.worker_rx,
            warning_rx: &mut self.warning_rx,
            ui_state_rx: &mut self.ui_state_rx,
            session_rx: &mut self.session_rx,
            ctx_state: &mut self.ctx_state,
        }
    }
}

fn make_ctx<'a>(
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

fn recv_command(worker_rx: &mut Option<mpsc::Receiver<WorkerCommand>>) -> Option<WorkerCommand> {
    worker_rx.as_mut()?.try_recv().ok()
}

fn take_warnings(warning_rx: &mut tokio::sync::broadcast::Receiver<WarningEvent>) -> Vec<String> {
    let mut out = Vec::new();
    while let Ok(event) = warning_rx.try_recv() {
        match event {
            WarningEvent::Message { message } => out.push(message),
            WarningEvent::TurnError { message } => out.push(message),
        }
    }
    out
}

fn take_ui_state(
    ui_state_rx: &mut tokio::sync::broadcast::Receiver<UiStateEvent>,
) -> Vec<UiStateEvent> {
    let mut out = Vec::new();
    while let Ok(event) = ui_state_rx.try_recv() {
        out.push(event);
    }
    out
}

fn take_session_events(
    session_rx: &mut tokio::sync::broadcast::Receiver<SessionEvent>,
) -> Vec<SessionEvent> {
    let mut out = Vec::new();
    while let Ok(event) = session_rx.try_recv() {
        out.push(event);
    }
    out
}

fn worker_rx_drop(h: &mut Harness) {
    h.worker_rx.take();
}

// ---------------------------------------------------------------------------
// handle_incoming tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn switch_model_dispatches_when_idle() {
    let mut h = Harness::new();
    let p = h.parts();
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::SwitchModel {
                spec: "test-model".to_string(),
            },
            &mut ctx,
        )
        .await;

    let cmd = recv_command(p.worker_rx).expect("command should be dispatched");
    match cmd {
        WorkerCommand::HandleUiRequest { request, .. } => {
            assert!(matches!(request, UiRequest::SwitchModel { spec } if spec == "test-model"));
        }
        _ => panic!("expected HandleUiRequest, got unexpected variant"),
    }
    assert!(p.stage.has_blocking_pending());
}

#[tokio::test]
async fn switch_model_queues_when_worker_active() {
    let mut h = Harness::new();
    let p = h.parts();
    p.ctx_state.worker_active = true;
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::SwitchModel {
                spec: "test-model".to_string(),
            },
            &mut ctx,
        )
        .await;

    assert!(
        recv_command(p.worker_rx).is_none(),
        "no command should be dispatched"
    );
    assert!(!p.stage.has_blocking_pending());
    let warnings = take_warnings(p.warning_rx);
    assert!(
        warnings
            .iter()
            .any(|w| w == "Model switch queued for next turn: test-model")
    );
}

#[tokio::test]
async fn switch_agent_dispatches_when_idle() {
    let mut h = Harness::new();
    let p = h.parts();
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::SwitchAgent {
                name: "research-agent".to_string(),
            },
            &mut ctx,
        )
        .await;

    let cmd = recv_command(p.worker_rx).expect("command should be dispatched");
    match cmd {
        WorkerCommand::HandleUiRequest { request, .. } => {
            assert!(matches!(request, UiRequest::SwitchAgent { name } if name == "research-agent"));
        }
        _ => panic!("expected HandleUiRequest, got unexpected variant"),
    }
    assert!(p.stage.has_blocking_pending());
}

#[tokio::test]
async fn switch_agent_queues_when_worker_active() {
    let mut h = Harness::new();
    let p = h.parts();
    p.ctx_state.worker_active = true;
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::SwitchAgent {
                name: "research-agent".to_string(),
            },
            &mut ctx,
        )
        .await;

    assert!(
        recv_command(p.worker_rx).is_none(),
        "no command should be dispatched"
    );
    let warnings = take_warnings(p.warning_rx);
    assert!(
        warnings
            .iter()
            .any(|w| w == "Agent switch queued for next turn: research-agent")
    );
}

#[tokio::test]
async fn switch_session_dispatches_when_idle() {
    let mut h = Harness::new();
    let p = h.parts();
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::SwitchSession {
                id: "session-1".to_string(),
            },
            &mut ctx,
        )
        .await;

    let cmd = recv_command(p.worker_rx).expect("command should be dispatched");
    match cmd {
        WorkerCommand::HandleUiRequest { request, .. } => {
            assert!(matches!(request, UiRequest::SwitchSession { id } if id == "session-1"));
        }
        _ => panic!("expected HandleUiRequest, got unexpected variant"),
    }
    assert!(p.stage.has_blocking_pending());
}

#[tokio::test]
async fn switch_session_rejects_when_worker_active() {
    let mut h = Harness::new();
    let p = h.parts();
    p.ctx_state.worker_active = true;
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::SwitchSession {
                id: "session-1".to_string(),
            },
            &mut ctx,
        )
        .await;

    assert!(
        recv_command(p.worker_rx).is_none(),
        "no command should be dispatched"
    );
    let warnings = take_warnings(p.warning_rx);
    assert!(
        warnings
            .iter()
            .any(|w| w == "Cannot switch session while worker is active")
    );
}

#[tokio::test]
async fn switch_session_ignored_when_blocking_pending() {
    let mut h = Harness::new();
    let p = h.parts();
    // First dispatch a model switch to occupy pending_blocking.
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::SwitchModel {
                spec: "test-model".to_string(),
            },
            &mut ctx,
        )
        .await;
    let _ = recv_command(p.worker_rx);

    // Now try a session switch — should be ignored silently.
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::SwitchSession {
                id: "session-1".to_string(),
            },
            &mut ctx,
        )
        .await;

    assert!(
        recv_command(p.worker_rx).is_none(),
        "no command should be dispatched"
    );
    assert!(
        take_warnings(p.warning_rx).is_empty(),
        "no warnings should be emitted"
    );
}

#[tokio::test]
async fn toggle_mcp_dispatches_immediately() {
    let mut h = Harness::new();
    let p = h.parts();
    p.ctx_state.worker_active = true; // MCP toggle works even when worker is active
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::ToggleMcp {
                server: "test-server".to_string(),
                enable: true,
            },
            &mut ctx,
        )
        .await;

    let cmd = recv_command(p.worker_rx).expect("command should be dispatched");
    match cmd {
        WorkerCommand::HandleUiRequest { request, .. } => {
            assert!(
                matches!(request, UiRequest::ToggleMcp { server, enable } if server == "test-server" && enable)
            );
        }
        _ => panic!("expected HandleUiRequest, got unexpected variant"),
    }
    assert!(p.stage.has_pending());
}

#[tokio::test]
async fn refresh_session_picker_dispatches_when_not_in_flight() {
    let mut h = Harness::new();
    let p = h.parts();
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(UiRequest::RefreshSessionPicker, &mut ctx)
        .await;

    let cmd = recv_command(p.worker_rx).expect("command should be dispatched");
    match cmd {
        WorkerCommand::HandleUiRequest { request, .. } => {
            assert!(matches!(request, UiRequest::RefreshSessionPicker));
        }
        _ => panic!("expected HandleUiRequest, got unexpected variant"),
    }
}

#[tokio::test]
async fn refresh_session_picker_skips_when_in_flight() {
    let mut h = Harness::new();
    let p = h.parts();
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(UiRequest::RefreshSessionPicker, &mut ctx)
        .await;
    let _ = recv_command(p.worker_rx);

    // Second refresh should be skipped.
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(UiRequest::RefreshSessionPicker, &mut ctx)
        .await;
    assert!(
        recv_command(p.worker_rx).is_none(),
        "no second command should be dispatched"
    );
}

#[tokio::test]
async fn switch_model_send_failure_warns() {
    let mut h = Harness::new();
    worker_rx_drop(&mut h);
    let p = h.parts();
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::SwitchModel {
                spec: "test-model".to_string(),
            },
            &mut ctx,
        )
        .await;

    let warnings = take_warnings(p.warning_rx);
    assert!(
        warnings
            .iter()
            .any(|w| w == "Model switch worker channel closed")
    );
    assert!(!p.stage.has_blocking_pending());
}

// ---------------------------------------------------------------------------
// handle_blocking_response tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn blocking_response_model_switch_success() {
    let mut h = Harness::new();
    let p = h.parts();
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::SwitchModel {
                spec: "test-model".to_string(),
            },
            &mut ctx,
        )
        .await;
    let _ = recv_command(p.worker_rx);

    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage.handle_blocking_response(
        UiRequestResponse::ModelSwitch(Ok(("test-model".to_string(), Some(128000)))),
        &mut ctx,
    );

    let ui_events = take_ui_state(p.ui_state_rx);
    assert!(
        ui_events
            .iter()
            .any(|e| matches!(e, UiStateEvent::SetActiveModelIdentity(id) if id == "test-model"))
    );
    assert!(
        ui_events
            .iter()
            .any(|e| matches!(e, UiStateEvent::SetContextWindowMaxTokens(Some(128000))))
    );
    let warnings = take_warnings(p.warning_rx);
    assert!(warnings.iter().any(|w| w == "Model switched: test-model"));
    assert!(!p.stage.has_blocking_pending());
}

#[tokio::test]
async fn blocking_response_model_switch_error() {
    let mut h = Harness::new();
    let p = h.parts();
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::SwitchModel {
                spec: "test-model".to_string(),
            },
            &mut ctx,
        )
        .await;
    let _ = recv_command(p.worker_rx);

    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage.handle_blocking_response(
        UiRequestResponse::ModelSwitch(Err("model not found".to_string())),
        &mut ctx,
    );

    let warnings = take_warnings(p.warning_rx);
    assert!(warnings.iter().any(|w| w == "model not found"));
    assert!(!p.stage.has_blocking_pending());
}

#[tokio::test]
async fn blocking_response_agent_switch_success() {
    let mut h = Harness::new();
    let p = h.parts();
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::SwitchAgent {
                name: "research-agent".to_string(),
            },
            &mut ctx,
        )
        .await;
    let _ = recv_command(p.worker_rx);

    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage.handle_blocking_response(
        UiRequestResponse::AgentSwitch(Ok((
            "research-agent".to_string(),
            "openai/gpt-4o".to_string(),
            Some(200000),
            Some("icon".to_string()),
        ))),
        &mut ctx,
    );

    let ui_events = take_ui_state(p.ui_state_rx);
    assert!(
        ui_events.iter().any(
            |e| matches!(e, UiStateEvent::SetActiveAgentIdentity(id) if id == "research-agent")
        )
    );
    assert!(
        ui_events
            .iter()
            .any(|e| matches!(e, UiStateEvent::SetActivePersonaIcon(Some(icon)) if icon == "icon"))
    );
    assert!(
        ui_events.iter().any(
            |e| matches!(e, UiStateEvent::SetActiveModelIdentity(id) if id == "openai/gpt-4o")
        )
    );
    assert!(
        ui_events
            .iter()
            .any(|e| matches!(e, UiStateEvent::SetContextWindowMaxTokens(Some(200000))))
    );
    let warnings = take_warnings(p.warning_rx);
    assert!(
        warnings
            .iter()
            .any(|w| w == "Agent switched to: research-agent")
    );
    assert!(!p.stage.has_blocking_pending());
}

#[tokio::test]
async fn blocking_response_agent_switch_error() {
    let mut h = Harness::new();
    let p = h.parts();
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::SwitchAgent {
                name: "research-agent".to_string(),
            },
            &mut ctx,
        )
        .await;
    let _ = recv_command(p.worker_rx);

    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage.handle_blocking_response(
        UiRequestResponse::AgentSwitch(Err("agent not found".to_string())),
        &mut ctx,
    );

    let warnings = take_warnings(p.warning_rx);
    assert!(warnings.iter().any(|w| w == "agent not found"));
    assert!(!p.stage.has_blocking_pending());
}

#[tokio::test]
async fn blocking_response_session_switch_success() {
    let mut h = Harness::new();
    let p = h.parts();
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::SwitchSession {
                id: "session-1".to_string(),
            },
            &mut ctx,
        )
        .await;
    let _ = recv_command(p.worker_rx);

    let snapshot = UiMessageSnapshot::new("user", "hello");
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage.handle_blocking_response(
        UiRequestResponse::SessionSwitch {
            id: "session-1".to_string(),
            result: Ok(vec![snapshot]),
        },
        &mut ctx,
    );

    let ui_events = take_ui_state(p.ui_state_rx);
    assert!(
        ui_events
            .iter()
            .any(|e| matches!(e, UiStateEvent::ClearTranscript))
    );
    assert!(ui_events.iter().any(
        |e| matches!(e, UiStateEvent::HydrateTranscript { messages, .. } if messages.len() == 1)
    ));
    let warnings = take_warnings(p.warning_rx);
    assert!(warnings.iter().any(|w| w == "Session switched"));
    let session_events = take_session_events(p.session_rx);
    assert!(session_events.iter().any(|e| matches!(e, SessionEvent::Switched { to_session_id, .. } if to_session_id == "session-1")));
    assert!(!p.stage.has_blocking_pending());
}

#[tokio::test]
async fn blocking_response_session_switch_error() {
    let mut h = Harness::new();
    let p = h.parts();
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::SwitchSession {
                id: "session-1".to_string(),
            },
            &mut ctx,
        )
        .await;
    let _ = recv_command(p.worker_rx);

    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage.handle_blocking_response(
        UiRequestResponse::SessionSwitch {
            id: "session-1".to_string(),
            result: Err("session not found".to_string()),
        },
        &mut ctx,
    );

    let warnings = take_warnings(p.warning_rx);
    assert!(warnings.iter().any(|w| w == "session not found"));
    assert!(!p.stage.has_blocking_pending());
}

// ---------------------------------------------------------------------------
// handle_concurrent_response tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn concurrent_response_mcp_toggle_success() {
    let mut h = Harness::new();
    let p = h.parts();
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::ToggleMcp {
                server: "test-server".to_string(),
                enable: true,
            },
            &mut ctx,
        )
        .await;
    let _ = recv_command(p.worker_rx);

    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage.handle_concurrent_response(
        UiRequestResponse::McpToggle {
            server: "test-server".to_string(),
            result: Ok(McpUsabilityState::Enabled),
            total: 5,
            server_count: 3,
            names_by_server: vec![("test-server".to_string(), vec!["tool1".to_string()])],
        },
        &mut ctx,
    );

    let ui_events = take_ui_state(p.ui_state_rx);
    assert!(ui_events.iter().any(|e| matches!(e, UiStateEvent::SetMcpServerState { server, state, error, total } if server == "test-server" && *state == McpUsabilityState::Enabled && error.is_none() && *total == 5)));
    assert!(ui_events.iter().any(|e| matches!(e, UiStateEvent::SetMcpVisibleToolCount { server, count } if server == "test-server" && *count == 3)));
    assert!(ui_events.iter().any(|e| matches!(e, UiStateEvent::SetMcpVisibleToolNames { server, names } if server == "test-server" && names == &vec!["tool1".to_string()])));
    assert!(!p.stage.has_pending());
}

#[tokio::test]
async fn concurrent_response_mcp_toggle_error() {
    let mut h = Harness::new();
    let p = h.parts();
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::ToggleMcp {
                server: "test-server".to_string(),
                enable: true,
            },
            &mut ctx,
        )
        .await;
    let _ = recv_command(p.worker_rx);

    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage.handle_concurrent_response(
        UiRequestResponse::McpToggle {
            server: "test-server".to_string(),
            result: Err("toggle failed".to_string()),
            total: 5,
            server_count: 3,
            names_by_server: Vec::new(),
        },
        &mut ctx,
    );

    let ui_events = take_ui_state(p.ui_state_rx);
    assert!(ui_events.iter().any(|e| matches!(e, UiStateEvent::SetMcpServerState { server, state, error, total } if server == "test-server" && *state == McpUsabilityState::Failed && error.as_deref() == Some("toggle failed") && *total == 5)));
    assert!(!p.stage.has_pending());
}

#[tokio::test]
async fn concurrent_response_session_refresh_success() {
    let mut h = Harness::new();
    let p = h.parts();
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(UiRequest::RefreshSessionPicker, &mut ctx)
        .await;
    let _ = recv_command(p.worker_rx);

    let session = SessionInfo {
        id: "session-1".to_string(),
        message_count: 3,
        last_active: chrono::Utc::now(),
        title: Some("Test".to_string()),
    };
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage.handle_concurrent_response(
        UiRequestResponse::SessionRefresh(Ok(vec![session])),
        &mut ctx,
    );

    let ui_events = take_ui_state(p.ui_state_rx);
    assert!(ui_events.iter().any(
        |e| matches!(e, UiStateEvent::SetSessionPickerOptions(sessions) if sessions.len() == 1)
    ));
    assert!(!p.stage.has_pending());
}

#[tokio::test]
async fn concurrent_response_session_refresh_error() {
    let mut h = Harness::new();
    let p = h.parts();
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(UiRequest::RefreshSessionPicker, &mut ctx)
        .await;
    let _ = recv_command(p.worker_rx);

    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage.handle_concurrent_response(
        UiRequestResponse::SessionRefresh(Err("refresh failed".to_string())),
        &mut ctx,
    );

    let warnings = take_warnings(p.warning_rx);
    assert!(warnings.iter().any(|w| w == "refresh failed"));
    assert!(!p.stage.has_pending());
}

#[tokio::test]
async fn switch_model_queue_last_write_wins() {
    let mut h = Harness::new();
    let p = h.parts();
    p.ctx_state.worker_active = true;
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );

    p.stage
        .handle_incoming(
            UiRequest::SwitchModel {
                spec: "first-model".to_string(),
            },
            &mut ctx,
        )
        .await;
    p.stage
        .handle_incoming(
            UiRequest::SwitchModel {
                spec: "second-model".to_string(),
            },
            &mut ctx,
        )
        .await;

    assert!(recv_command(p.worker_rx).is_none());
    assert!(!p.stage.has_blocking_pending());

    let warnings = take_warnings(p.warning_rx);
    assert!(
        warnings
            .iter()
            .any(|w| w == "Model switch queued for next turn: second-model")
    );

    p.ctx_state.worker_active = false;
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage.drain_queued(&mut ctx).await;

    let cmd = recv_command(p.worker_rx).expect("queued command should be dispatched");
    match cmd {
        WorkerCommand::HandleUiRequest { request, .. } => {
            assert!(
                matches!(request, UiRequest::SwitchModel { spec } if spec == "second-model"),
                "last queued model switch should win"
            );
        }
        _ => panic!("expected HandleUiRequest, got unexpected variant"),
    }
}

#[tokio::test]
async fn switch_agent_queue_last_write_wins() {
    let mut h = Harness::new();
    let p = h.parts();
    p.ctx_state.worker_active = true;
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );

    p.stage
        .handle_incoming(
            UiRequest::SwitchAgent {
                name: "agent-a".to_string(),
            },
            &mut ctx,
        )
        .await;
    p.stage
        .handle_incoming(
            UiRequest::SwitchAgent {
                name: "agent-b".to_string(),
            },
            &mut ctx,
        )
        .await;

    assert!(recv_command(p.worker_rx).is_none());
    assert!(!p.stage.has_blocking_pending());

    let warnings = take_warnings(p.warning_rx);
    assert!(
        warnings
            .iter()
            .any(|w| w == "Agent switch queued for next turn: agent-b")
    );

    p.ctx_state.worker_active = false;
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage.drain_queued(&mut ctx).await;

    let cmd = recv_command(p.worker_rx).expect("queued command should be dispatched");
    match cmd {
        WorkerCommand::HandleUiRequest { request, .. } => {
            assert!(
                matches!(request, UiRequest::SwitchAgent { name } if name == "agent-b"),
                "last queued agent switch should win"
            );
        }
        _ => panic!("expected HandleUiRequest, got unexpected variant"),
    }
}

// ---------------------------------------------------------------------------
// drain_queued tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn drain_queued_dispatches_when_idle() {
    let mut h = Harness::new();
    let p = h.parts();
    // Queue a model switch while worker is active.
    p.ctx_state.worker_active = true;
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::SwitchModel {
                spec: "test-model".to_string(),
            },
            &mut ctx,
        )
        .await;
    assert!(recv_command(p.worker_rx).is_none());

    // Now worker becomes idle — drain should dispatch.
    p.ctx_state.worker_active = false;
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage.drain_queued(&mut ctx).await;

    let cmd = recv_command(p.worker_rx).expect("queued command should be dispatched");
    match cmd {
        WorkerCommand::HandleUiRequest { request, .. } => {
            assert!(matches!(request, UiRequest::SwitchModel { spec } if spec == "test-model"));
        }
        _ => panic!("expected HandleUiRequest, got unexpected variant"),
    }
    assert!(p.stage.has_blocking_pending());
}

#[tokio::test]
async fn drain_queued_does_nothing_when_worker_active() {
    let mut h = Harness::new();
    let p = h.parts();
    p.ctx_state.worker_active = true;
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::SwitchModel {
                spec: "test-model".to_string(),
            },
            &mut ctx,
        )
        .await;
    assert!(recv_command(p.worker_rx).is_none());

    // Still active — drain should not dispatch.
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage.drain_queued(&mut ctx).await;
    assert!(recv_command(p.worker_rx).is_none());
}

#[tokio::test]
async fn drain_queued_does_nothing_when_blocking_pending() {
    let mut h = Harness::new();
    let p = h.parts();
    // Dispatch a model switch to occupy pending_blocking.
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::SwitchModel {
                spec: "test-model".to_string(),
            },
            &mut ctx,
        )
        .await;
    let _ = recv_command(p.worker_rx);

    // Queue another while blocking is pending.
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::SwitchAgent {
                name: "research-agent".to_string(),
            },
            &mut ctx,
        )
        .await;
    assert!(recv_command(p.worker_rx).is_none());

    // Drain should not dispatch because blocking is still pending.
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage.drain_queued(&mut ctx).await;
    assert!(recv_command(p.worker_rx).is_none());
}

// ---------------------------------------------------------------------------
// has_pending tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn has_pending_reflects_blocking_and_concurrent() {
    let mut h = Harness::new();
    let p = h.parts();

    // Initially no pending.
    assert!(!p.stage.has_blocking_pending());
    assert!(!p.stage.has_pending());

    // Dispatch a blocking request.
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::SwitchModel {
                spec: "test-model".to_string(),
            },
            &mut ctx,
        )
        .await;
    let _ = recv_command(p.worker_rx);
    assert!(p.stage.has_blocking_pending());
    assert!(p.stage.has_pending());

    // Dispatch a concurrent request.
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage
        .handle_incoming(
            UiRequest::ToggleMcp {
                server: "test-server".to_string(),
                enable: true,
            },
            &mut ctx,
        )
        .await;
    let _ = recv_command(p.worker_rx);
    assert!(p.stage.has_pending());

    // Clear blocking.
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage.handle_blocking_response(
        UiRequestResponse::ModelSwitch(Ok(("test-model".to_string(), None))),
        &mut ctx,
    );
    assert!(!p.stage.has_blocking_pending());
    assert!(p.stage.has_pending()); // concurrent still pending

    // Clear concurrent.
    let mut ctx = make_ctx(
        p.worker_tx,
        p.blocking_tx,
        p.concurrent_tx,
        p.bus,
        p.ctx_state,
    );
    p.stage.handle_concurrent_response(
        UiRequestResponse::McpToggle {
            server: "test-server".to_string(),
            result: Ok(McpUsabilityState::Enabled),
            total: 0,
            server_count: 0,
            names_by_server: Vec::new(),
        },
        &mut ctx,
    );
    assert!(!p.stage.has_pending());
}
