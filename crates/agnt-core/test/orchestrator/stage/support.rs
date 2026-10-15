//! Shared harness for the stage test modules.
//!
//! Holds the imports and the harness every topical stage test file needs.
//! Sub-files pull them in with `use super::*;`.

pub(super) use agnt_a2a::InMemoryTaskStore;
pub(super) use nu_protocol::{LabeledError, Span, Value};
pub(super) use tokio::sync::mpsc;

pub(super) use crate::bus::{
    Bus, SessionEvent, SessionRx, TurnEvent, UiEventRx, UiStateRx, create_bus,
};
pub(super) use crate::conversation::runtime::PendingPermissions;
pub(super) use crate::orchestrator::stages::session::SessionStage;
pub(super) use crate::orchestrator::stages::ui_request::UiRequestStage;
pub(super) use crate::orchestrator::stages::{
    OrchestrationContext, SessionHandler, UiRequestHandler,
};
pub(super) use crate::orchestrator::turn_outcome::TurnOutcome;
pub(super) use crate::orchestrator::{UiRequest, UiRequestResponse, UiStateEvent, WorkerCommand};
pub(super) use crate::protocol::contracts::{McpUsabilityState, UiMessageSnapshot};
pub(super) use crate::protocol::event::UiEvent;
pub(super) use crate::session::SessionInfo;

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

pub(super) struct CtxState {
    pub(super) worker_active: bool,
    pending: Option<PendingPermissions>,
    pub(super) active_external_prompt: Option<String>,
    pub(super) active_external_task_id: Option<String>,
    pending_external_cancel: Option<String>,
    pending_a2a_task_id: Option<String>,
    task_store: Option<std::sync::Arc<InMemoryTaskStore>>,
}

pub(super) struct HarnessParts<'a> {
    pub(super) stage: &'a mut UiRequestStage,
    pub(super) worker_tx: &'a mpsc::Sender<WorkerCommand>,
    pub(super) blocking_tx: &'a mpsc::Sender<UiRequestResponse>,
    pub(super) concurrent_tx: &'a mpsc::Sender<UiRequestResponse>,
    pub(super) bus: &'a Bus,
    pub(super) worker_rx: &'a mut Option<mpsc::Receiver<WorkerCommand>>,
    pub(super) ui_event_rx: &'a mut UiEventRx,
    pub(super) ui_state_rx: &'a mut UiStateRx,
    pub(super) session_rx: &'a mut SessionRx,
    pub(super) state: &'a mut CtxState,
}

pub(super) struct Harness {
    stage: UiRequestStage,
    worker_tx: mpsc::Sender<WorkerCommand>,
    worker_rx: Option<mpsc::Receiver<WorkerCommand>>,
    blocking_tx: mpsc::Sender<UiRequestResponse>,
    concurrent_tx: mpsc::Sender<UiRequestResponse>,
    bus: Bus,
    ui_event_rx: UiEventRx,
    ui_state_rx: UiStateRx,
    session_rx: SessionRx,
    state: CtxState,
}

impl Harness {
    pub(super) fn new() -> Self {
        let (worker_tx, worker_rx) = mpsc::channel::<WorkerCommand>(256);
        let (blocking_tx, _blocking_rx) = mpsc::channel::<UiRequestResponse>(256);
        let (concurrent_tx, _concurrent_rx) = mpsc::channel::<UiRequestResponse>(256);
        let bus = create_bus();
        let ui_event_rx = bus.ui_event().subscribe();
        let ui_state_rx = bus.ui_state().subscribe();
        let session_rx = bus.session().subscribe();
        Self {
            stage: UiRequestStage::new(0),
            worker_tx,
            worker_rx: Some(worker_rx),
            blocking_tx,
            concurrent_tx,
            ui_event_rx,
            ui_state_rx,
            session_rx,
            bus,
            state: CtxState {
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

    pub(super) fn parts(&mut self) -> HarnessParts<'_> {
        HarnessParts {
            stage: &mut self.stage,
            worker_tx: &self.worker_tx,
            blocking_tx: &self.blocking_tx,
            concurrent_tx: &self.concurrent_tx,
            bus: &self.bus,
            worker_rx: &mut self.worker_rx,
            ui_event_rx: &mut self.ui_event_rx,
            ui_state_rx: &mut self.ui_state_rx,
            session_rx: &mut self.session_rx,
            state: &mut self.state,
        }
    }
}

pub(super) fn make_ctx<'a>(
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

pub(super) fn recv_command(
    worker_rx: &mut Option<mpsc::Receiver<WorkerCommand>>,
) -> Option<WorkerCommand> {
    worker_rx.as_mut()?.try_recv().ok()
}

pub(super) fn take_warnings(ui_event_rx: &mut UiEventRx) -> Vec<String> {
    let mut out = Vec::new();
    while let Ok(event) = ui_event_rx.try_recv() {
        match event {
            UiEvent::Warning { message } => out.push(message),
            UiEvent::TurnError { message } => out.push(message),
            _ => {}
        }
    }
    out
}

pub(super) fn take_ui_state(ui_state_rx: &mut UiStateRx) -> Vec<UiStateEvent> {
    let mut out = Vec::new();
    while let Ok(event) = ui_state_rx.try_recv() {
        out.push(event);
    }
    out
}

pub(super) fn take_session_events(session_rx: &mut SessionRx) -> Vec<SessionEvent> {
    let mut out = Vec::new();
    while let Ok(event) = session_rx.try_recv() {
        out.push(event);
    }
    out
}

pub(super) fn worker_rx_drop(h: &mut Harness) {
    h.worker_rx.take();
}

pub(super) fn session_info(id: &str) -> SessionInfo {
    SessionInfo {
        id: id.to_string(),
        message_count: 0,
        last_active: chrono::Utc::now(),
        title: None,
    }
}
