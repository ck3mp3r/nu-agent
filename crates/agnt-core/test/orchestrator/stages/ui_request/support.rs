//! Shared harness for the ui_request stage test modules.
//!
//! Holds the imports and the harness every topical ui_request test file needs.
//! Sub-files pull them in with `use super::*;`.

pub(super) use agnt_a2a::InMemoryTaskStore;
pub(super) use nu_protocol::Span;
pub(super) use tokio::sync::mpsc;

pub(super) use crate::bus::{Bus, SessionEvent, SessionRx, UiEventRx, UiStateRx, create_bus};
pub(super) use crate::conversation::runtime::PendingPermissions;
pub(super) use crate::orchestrator::stages::ui_request::UiRequestStage;
pub(super) use crate::orchestrator::stages::{OrchestrationContext, UiRequestHandler};
pub(super) use crate::orchestrator::{UiRequest, UiRequestResponse, UiStateEvent, WorkerCommand};
pub(super) use crate::protocol::contracts::{McpUsabilityState, UiMessageSnapshot};
pub(super) use crate::protocol::events::SystemEvent;
pub(super) use crate::session::SessionInfo;

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Mutable per-test state that the `UiRequestStage` reads and writes through
/// `OrchestrationContext`. Kept separate from the channels so `ctx()` can
/// borrow it disjointly from `stage` and the receivers.
pub(super) struct CtxState {
    pub(super) worker_active: bool,
    pending: Option<PendingPermissions>,
    active_external_prompt: Option<String>,
    active_external_task_id: Option<String>,
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
    pub(super) ctx_state: &'a mut CtxState,
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
    ctx_state: CtxState,
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

    /// Decompose the harness into fully disjoint borrows so a long-lived
    /// `OrchestrationContext` (built via the free `ctx()` below) does not
    /// conflict with the receivers used by `recv_*`/`take_*`.
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
            ctx_state: &mut self.ctx_state,
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
        if let SystemEvent::SystemMessage { text, .. } = event {
            out.push(text);
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
