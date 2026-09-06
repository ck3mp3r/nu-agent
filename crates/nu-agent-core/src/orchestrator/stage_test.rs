use nu_protocol::{LabeledError, Span};
use tokio::sync::mpsc;

use crate::bus::{Bus, SessionEvent, SessionRx, UiStateRx, WarningEvent, WarningRx, create_bus};
use crate::conversation::runtime::PendingPermissions;
use crate::orchestrator::stages::session::SessionStage;
use crate::orchestrator::stages::ui_request::UiRequestStage;
use crate::orchestrator::stages::{OrchestrationContext, SessionHandler, UiRequestHandler};
use crate::orchestrator::turn_outcome::TurnOutcome;
use crate::orchestrator::{UiRequest, UiRequestResponse, UiStateEvent, WorkerCommand};
use crate::protocol::contracts::{McpUsabilityState, UiMessageSnapshot};
use crate::session::SessionInfo;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

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
    warning_rx: &'a mut WarningRx,
    ui_state_rx: &'a mut UiStateRx,
    session_rx: &'a mut SessionRx,
    state: &'a mut CtxState,
}

struct Harness {
    stage: UiRequestStage,
    worker_tx: mpsc::Sender<WorkerCommand>,
    worker_rx: Option<mpsc::Receiver<WorkerCommand>>,
    blocking_tx: mpsc::Sender<UiRequestResponse>,
    concurrent_tx: mpsc::Sender<UiRequestResponse>,
    bus: Bus,
    warning_rx: WarningRx,
    ui_state_rx: UiStateRx,
    session_rx: SessionRx,
    state: CtxState,
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
            state: CtxState {
                worker_active: false,
                pending: None,
                active_external_prompt: None,
                active_external_task_id: None,
                pending_external_cancel: None,
            },
        }
    }

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
            state: &mut self.state,
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

fn take_warnings(warning_rx: &mut WarningRx) -> Vec<String> {
    let mut out = Vec::new();
    while let Ok(event) = warning_rx.try_recv() {
        match event {
            WarningEvent::Message { message } => out.push(message),
            WarningEvent::TurnError { message } => out.push(message),
        }
    }
    out
}

fn take_ui_state(ui_state_rx: &mut UiStateRx) -> Vec<UiStateEvent> {
    let mut out = Vec::new();
    while let Ok(event) = ui_state_rx.try_recv() {
        out.push(event);
    }
    out
}

fn take_session_events(session_rx: &mut SessionRx) -> Vec<SessionEvent> {
    let mut out = Vec::new();
    while let Ok(event) = session_rx.try_recv() {
        out.push(event);
    }
    out
}

fn worker_rx_drop(h: &mut Harness) {
    h.worker_rx.take();
}

fn session_info(id: &str) -> SessionInfo {
    SessionInfo {
        id: id.to_string(),
        message_count: 0,
        last_active: chrono::Utc::now(),
        title: None,
    }
}

// ---------------------------------------------------------------------------
// Model switch
// ---------------------------------------------------------------------------

#[tokio::test]
async fn model_switch_send_failure_emits_warning() {
    let mut h = Harness::new();
    worker_rx_drop(&mut h);
    let HarnessParts {
        stage,
        worker_tx,
        blocking_tx,
        concurrent_tx,
        bus,
        worker_rx: _,
        warning_rx,
        ui_state_rx: _,
        session_rx: _,
        state,
    } = h.parts();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_incoming(
            UiRequest::SwitchModel {
                spec: "gpt-4o".to_string(),
            },
            &mut ctx,
        )
        .await;

    let warnings = take_warnings(warning_rx);
    assert!(
        warnings
            .iter()
            .any(|w| w == "Model switch worker channel closed")
    );
    assert!(!stage.has_blocking_pending());
}

#[tokio::test]
async fn model_switch_error_response_emits_warning() -> Result<()> {
    let mut h = Harness::new();
    let HarnessParts {
        stage,
        worker_tx,
        blocking_tx,
        concurrent_tx,
        bus,
        worker_rx,
        warning_rx,
        ui_state_rx: _,
        session_rx: _,
        state,
    } = h.parts();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_incoming(
            UiRequest::SwitchModel {
                spec: "gpt-4o".to_string(),
            },
            &mut ctx,
        )
        .await;

    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_blocking_response(
            UiRequestResponse::ModelSwitch(Err("model failed".to_string())),
            &mut ctx,
        )
        .await;

    let warnings = take_warnings(warning_rx);
    assert!(warnings.iter().any(|w| w == "model failed"));
    let cmd = recv_command(worker_rx).ok_or("HandleUiRequest should be dispatched")?;
    assert!(
        matches!(cmd, WorkerCommand::HandleUiRequest { .. }),
        "expected HandleUiRequest command"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Agent switch
// ---------------------------------------------------------------------------

#[tokio::test]
async fn agent_switch_send_failure_emits_warning() {
    let mut h = Harness::new();
    worker_rx_drop(&mut h);
    let HarnessParts {
        stage,
        worker_tx,
        blocking_tx,
        concurrent_tx,
        bus,
        worker_rx: _,
        warning_rx,
        ui_state_rx: _,
        session_rx: _,
        state,
    } = h.parts();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_incoming(
            UiRequest::SwitchAgent {
                name: "research".to_string(),
            },
            &mut ctx,
        )
        .await;

    let warnings = take_warnings(warning_rx);
    assert!(
        warnings
            .iter()
            .any(|w| w == "Agent switch worker channel closed")
    );
    assert!(!stage.has_blocking_pending());
}

#[tokio::test]
async fn agent_switch_error_response_emits_warning() -> Result<()> {
    let mut h = Harness::new();
    let HarnessParts {
        stage,
        worker_tx,
        blocking_tx,
        concurrent_tx,
        bus,
        worker_rx,
        warning_rx,
        ui_state_rx: _,
        session_rx: _,
        state,
    } = h.parts();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_incoming(
            UiRequest::SwitchAgent {
                name: "research".to_string(),
            },
            &mut ctx,
        )
        .await;

    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_blocking_response(
            UiRequestResponse::AgentSwitch(Err("agent failed".to_string())),
            &mut ctx,
        )
        .await;

    let warnings = take_warnings(warning_rx);
    assert!(warnings.iter().any(|w| w == "agent failed"));
    let cmd = recv_command(worker_rx).ok_or("HandleUiRequest should be dispatched")?;
    assert!(
        matches!(cmd, WorkerCommand::HandleUiRequest { .. }),
        "expected HandleUiRequest command"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Session switch
// ---------------------------------------------------------------------------

#[tokio::test]
async fn session_switch_while_worker_active_is_rejected_and_not_queued() {
    let mut h = Harness::new();
    let HarnessParts {
        stage,
        worker_tx,
        blocking_tx,
        concurrent_tx,
        bus,
        worker_rx,
        warning_rx,
        ui_state_rx: _,
        session_rx: _,
        state,
    } = h.parts();
    state.worker_active = true;
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_incoming(
            UiRequest::SwitchSession {
                id: "sess-1".to_string(),
            },
            &mut ctx,
        )
        .await;

    let warnings = take_warnings(warning_rx);
    assert!(
        warnings
            .iter()
            .any(|w| w == "Cannot switch session while worker is active")
    );
    assert!(
        recv_command(worker_rx).is_none(),
        "no worker command should be dispatched"
    );
}

#[tokio::test]
async fn session_switch_while_pending_is_ignored() {
    let mut h = Harness::new();
    let HarnessParts {
        stage,
        worker_tx,
        blocking_tx,
        concurrent_tx,
        bus,
        worker_rx,
        warning_rx,
        ui_state_rx: _,
        session_rx: _,
        state,
    } = h.parts();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_incoming(
            UiRequest::SwitchSession {
                id: "sess-1".to_string(),
            },
            &mut ctx,
        )
        .await;
    let _ = recv_command(worker_rx);

    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_incoming(
            UiRequest::SwitchSession {
                id: "sess-2".to_string(),
            },
            &mut ctx,
        )
        .await;

    assert!(
        recv_command(worker_rx).is_none(),
        "only one session switch should be dispatched"
    );
    let warnings = take_warnings(warning_rx);
    assert!(
        !warnings.iter().any(|w| w.contains("sess-2")),
        "no warning should mention the ignored second request, got {warnings:?}"
    );
    assert!(stage.has_blocking_pending());
}

#[tokio::test]
async fn session_switch_success_clears_and_hydrates_transcript() {
    let mut h = Harness::new();
    let HarnessParts {
        stage,
        worker_tx,
        blocking_tx,
        concurrent_tx,
        bus,
        worker_rx,
        warning_rx: _,
        ui_state_rx,
        session_rx,
        state,
    } = h.parts();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_incoming(
            UiRequest::SwitchSession {
                id: "sess-1".to_string(),
            },
            &mut ctx,
        )
        .await;
    let _ = recv_command(worker_rx);

    let snapshot = UiMessageSnapshot::new("user", "hello");
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_blocking_response(
            UiRequestResponse::SessionSwitch {
                id: "sess-1".to_string(),
                result: Ok(vec![snapshot]),
            },
            &mut ctx,
        )
        .await;

    let ui_events = take_ui_state(ui_state_rx);
    assert!(
        ui_events
            .iter()
            .any(|e| matches!(e, UiStateEvent::ClearTranscript)),
        "transcript must be cleared"
    );
    assert!(
        ui_events.iter().any(|e| {
            matches!(e, UiStateEvent::HydrateTranscript { messages, .. } if messages.len() == 1)
        }),
        "transcript must be re-hydrated"
    );
    let session_events = take_session_events(session_rx);
    assert!(
        session_events.iter().any(|e| {
            matches!(e, SessionEvent::Switched { from_session_id: None, to_session_id } if to_session_id == "sess-1")
        })
    );
    assert!(!stage.has_blocking_pending());
}

#[tokio::test]
async fn session_switch_error_emits_warning() {
    let mut h = Harness::new();
    let HarnessParts {
        stage,
        worker_tx,
        blocking_tx,
        concurrent_tx,
        bus,
        worker_rx,
        warning_rx,
        ui_state_rx: _,
        session_rx: _,
        state,
    } = h.parts();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_incoming(
            UiRequest::SwitchSession {
                id: "sess-1".to_string(),
            },
            &mut ctx,
        )
        .await;
    let _ = recv_command(worker_rx);

    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_blocking_response(
            UiRequestResponse::SessionSwitch {
                id: "sess-1".to_string(),
                result: Err("session load failed".to_string()),
            },
            &mut ctx,
        )
        .await;

    let warnings = take_warnings(warning_rx);
    assert!(warnings.iter().any(|w| w == "session load failed"));
    assert!(!stage.has_blocking_pending());
}

#[tokio::test]
async fn session_switch_send_failure_emits_warning() {
    let mut h = Harness::new();
    worker_rx_drop(&mut h);
    let HarnessParts {
        stage,
        worker_tx,
        blocking_tx,
        concurrent_tx,
        bus,
        worker_rx: _,
        warning_rx,
        ui_state_rx: _,
        session_rx: _,
        state,
    } = h.parts();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_incoming(
            UiRequest::SwitchSession {
                id: "sess-1".to_string(),
            },
            &mut ctx,
        )
        .await;

    let warnings = take_warnings(warning_rx);
    assert!(
        warnings
            .iter()
            .any(|w| w == "Session switch worker channel closed")
    );
    assert!(!stage.has_blocking_pending());
}

// ---------------------------------------------------------------------------
// Session picker refresh
// ---------------------------------------------------------------------------

#[tokio::test]
async fn session_picker_launch_dispatches_refresh_command() -> Result<()> {
    let mut h = Harness::new();
    let HarnessParts {
        stage,
        worker_tx,
        blocking_tx,
        concurrent_tx,
        bus,
        worker_rx,
        warning_rx: _,
        ui_state_rx: _,
        session_rx: _,
        state,
    } = h.parts();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_incoming(UiRequest::RefreshSessionPicker, &mut ctx)
        .await;

    let cmd = recv_command(worker_rx).ok_or("RefreshSessionPicker should be dispatched")?;
    assert!(
        matches!(
            cmd,
            WorkerCommand::HandleUiRequest {
                request: UiRequest::RefreshSessionPicker,
                ..
            }
        ),
        "expected RefreshSessionPicker command"
    );
    Ok(())
}

#[tokio::test]
async fn session_picker_refresh_dispatched_when_no_pending() -> Result<()> {
    let mut h = Harness::new();
    let HarnessParts {
        stage,
        worker_tx,
        blocking_tx,
        concurrent_tx,
        bus,
        worker_rx,
        warning_rx: _,
        ui_state_rx: _,
        session_rx: _,
        state,
    } = h.parts();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_incoming(UiRequest::RefreshSessionPicker, &mut ctx)
        .await;

    let cmd = recv_command(worker_rx).ok_or("RefreshSessionPicker should be dispatched")?;
    assert!(
        matches!(
            cmd,
            WorkerCommand::HandleUiRequest {
                request: UiRequest::RefreshSessionPicker,
                ..
            }
        ),
        "expected RefreshSessionPicker command when no refresh is pending"
    );
    Ok(())
}

#[tokio::test]
async fn session_picker_refresh_in_flight_skips_new_command() {
    let mut h = Harness::new();
    let HarnessParts {
        stage,
        worker_tx,
        blocking_tx,
        concurrent_tx,
        bus,
        worker_rx,
        warning_rx: _,
        ui_state_rx: _,
        session_rx: _,
        state,
    } = h.parts();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_incoming(UiRequest::RefreshSessionPicker, &mut ctx)
        .await;
    let _ = recv_command(worker_rx);

    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_incoming(UiRequest::RefreshSessionPicker, &mut ctx)
        .await;

    assert!(
        recv_command(worker_rx).is_none(),
        "only one RefreshSessionPicker should be dispatched while one is pending"
    );
    assert!(stage.has_pending());
}

#[tokio::test]
async fn session_picker_refresh_success_sets_options() {
    let mut h = Harness::new();
    let HarnessParts {
        stage,
        worker_tx,
        blocking_tx,
        concurrent_tx,
        bus,
        worker_rx,
        warning_rx: _,
        ui_state_rx,
        session_rx: _,
        state,
    } = h.parts();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_incoming(UiRequest::RefreshSessionPicker, &mut ctx)
        .await;
    let _ = recv_command(worker_rx);

    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_concurrent_response(
            UiRequestResponse::SessionRefresh(Ok(vec![session_info("sess-1")])),
            &mut ctx,
        )
        .await;

    let ui_events = take_ui_state(ui_state_rx);
    assert!(
        ui_events.iter().any(|e| {
            matches!(e, UiStateEvent::SetSessionPickerOptions(sessions) if sessions.len() == 1)
        }),
        "options must be set"
    );
    assert!(
        ui_events.iter().any(|e| {
            matches!(e, UiStateEvent::SetSessionPickerOptions(sessions) if sessions.len() == 1 && sessions[0].id == "sess-1")
        }),
        "options[0] must be sess-1"
    );
    assert!(!stage.has_pending());
}

#[tokio::test]
async fn session_picker_refresh_error_warns() {
    let mut h = Harness::new();
    let HarnessParts {
        stage,
        worker_tx,
        blocking_tx,
        concurrent_tx,
        bus,
        worker_rx,
        warning_rx,
        ui_state_rx: _,
        session_rx: _,
        state,
    } = h.parts();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_incoming(UiRequest::RefreshSessionPicker, &mut ctx)
        .await;
    let _ = recv_command(worker_rx);

    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_concurrent_response(
            UiRequestResponse::SessionRefresh(Err("refresh failed".to_string())),
            &mut ctx,
        )
        .await;

    let warnings = take_warnings(warning_rx);
    assert!(warnings.iter().any(|w| w == "refresh failed"));
    assert!(!stage.has_pending());
}

#[tokio::test]
async fn session_picker_refresh_send_failure_warns() {
    let mut h = Harness::new();
    worker_rx_drop(&mut h);
    let HarnessParts {
        stage,
        worker_tx,
        blocking_tx,
        concurrent_tx,
        bus,
        worker_rx: _,
        warning_rx,
        ui_state_rx: _,
        session_rx: _,
        state,
    } = h.parts();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_incoming(UiRequest::RefreshSessionPicker, &mut ctx)
        .await;

    let warnings = take_warnings(warning_rx);
    assert!(warnings.iter().any(|w| w == "Worker channel closed"));
    assert!(!stage.has_pending());
}

// ---------------------------------------------------------------------------
// Cross-cutting
// ---------------------------------------------------------------------------

#[tokio::test]
async fn worker_result_processed_before_model_switch_result() -> Result<()> {
    let mut h = Harness::new();
    let HarnessParts {
        stage,
        worker_tx,
        blocking_tx,
        concurrent_tx,
        bus,
        worker_rx: _,
        warning_rx,
        ui_state_rx: _,
        session_rx: _,
        state,
    } = h.parts();
    let mut session = SessionStage;
    state.worker_active = true;
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    session
        .handle_outcome(
            TurnOutcome::Error(LabeledError::new("turn failed")),
            &mut ctx,
        )
        .await;

    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_blocking_response(
            UiRequestResponse::ModelSwitch(Err("model failed".to_string())),
            &mut ctx,
        )
        .await;

    let warnings = take_warnings(warning_rx);
    let session_idx = warnings
        .iter()
        .position(|w| w.starts_with("Turn failed:"))
        .ok_or("session turn error warning should be present")?;
    let model_idx = warnings
        .iter()
        .position(|w| w == "model failed")
        .ok_or("model switch error warning should be present")?;
    assert!(
        session_idx < model_idx,
        "session result must be processed before model result, got {warnings:?}"
    );
    Ok(())
}

#[tokio::test]
async fn pending_model_switch_skips_slash_stage() {
    let mut h = Harness::new();
    let HarnessParts {
        stage,
        worker_tx,
        blocking_tx,
        concurrent_tx,
        bus,
        worker_rx,
        warning_rx: _,
        ui_state_rx: _,
        session_rx: _,
        state,
    } = h.parts();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_incoming(
            UiRequest::SwitchModel {
                spec: "gpt-4o".to_string(),
            },
            &mut ctx,
        )
        .await;
    let _ = recv_command(worker_rx);

    assert!(stage.has_blocking_pending());
    assert!(
        recv_command(worker_rx).is_none(),
        "no ExecuteTurn command should be dispatched"
    );
}

#[tokio::test]
async fn concurrent_in_flight_rules_allow_multiple_mcp_toggles() {
    let mut h = Harness::new();
    let HarnessParts {
        stage,
        worker_tx,
        blocking_tx,
        concurrent_tx,
        bus,
        worker_rx,
        warning_rx: _,
        ui_state_rx: _,
        session_rx: _,
        state,
    } = h.parts();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_incoming(
            UiRequest::ToggleMcp {
                server: "gh".to_string(),
                enable: false,
            },
            &mut ctx,
        )
        .await;
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_incoming(
            UiRequest::ToggleMcp {
                server: "fs".to_string(),
                enable: true,
            },
            &mut ctx,
        )
        .await;

    let mut mcp_toggles = 0;
    while let Some(cmd) = recv_command(worker_rx) {
        if matches!(
            cmd,
            WorkerCommand::HandleUiRequest {
                request: UiRequest::ToggleMcp { .. },
                ..
            }
        ) {
            mcp_toggles += 1;
        }
    }
    assert_eq!(
        mcp_toggles, 2,
        "multiple MCP toggles may be in-flight simultaneously"
    );

    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_concurrent_response(
            UiRequestResponse::McpToggle {
                server: "gh".to_string(),
                result: Ok(McpUsabilityState::Disabled),
                total: 0,
                server_count: 0,
                names_by_server: vec![],
            },
            &mut ctx,
        )
        .await;
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_concurrent_response(
            UiRequestResponse::McpToggle {
                server: "fs".to_string(),
                result: Ok(McpUsabilityState::Enabled),
                total: 0,
                server_count: 0,
                names_by_server: vec![],
            },
            &mut ctx,
        )
        .await;

    assert!(!stage.has_pending());
}

#[tokio::test]
async fn session_switch_success_rehydrates_transcript_from_snapshots() {
    let mut h = Harness::new();
    let HarnessParts {
        stage,
        worker_tx,
        blocking_tx,
        concurrent_tx,
        bus,
        worker_rx,
        warning_rx: _,
        ui_state_rx,
        session_rx: _,
        state,
    } = h.parts();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_incoming(
            UiRequest::SwitchSession {
                id: "sess-1".to_string(),
            },
            &mut ctx,
        )
        .await;
    let _ = recv_command(worker_rx);

    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_blocking_response(
            UiRequestResponse::SessionSwitch {
                id: "sess-1".to_string(),
                result: Ok(vec![
                    UiMessageSnapshot::new("user", "first"),
                    UiMessageSnapshot::new("assistant", "second"),
                ]),
            },
            &mut ctx,
        )
        .await;

    let ui_events = take_ui_state(ui_state_rx);
    assert!(
        ui_events
            .iter()
            .any(|e| matches!(e, UiStateEvent::ClearTranscript)),
        "transcript must be cleared"
    );
    assert!(
        ui_events.iter().any(|e| {
            matches!(e, UiStateEvent::HydrateTranscript { messages, .. } if messages.len() == 2)
        }),
        "transcript must be re-hydrated from all snapshots"
    );
}
