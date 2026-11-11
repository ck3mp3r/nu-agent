//! Shared harness for the stage test modules.
//!
//! Holds the imports and the harness every topical stage test file needs.
//! Sub-files pull them in with `use super::*;`.

pub(super) use nu_protocol::{LabeledError, Span, Value};
pub(super) use tokio::sync::mpsc;

pub(super) use crate::bus::{Bus, LifecycleRx, NarrationRx};
pub(super) use crate::catalog::PickerCatalog;
pub(super) use crate::conversation::runtime::PendingPermissions;
pub(super) use crate::orchestrator::events::{UiRequest, UiRequestResponse, WorkerCommand};
pub(super) use crate::orchestrator::stages::session::SessionStage;
pub(super) use crate::orchestrator::stages::ui_request::UiRequestStage;
pub(super) use crate::orchestrator::stages::{
    OrchestrationContext, SessionHandler, UiRequestHandler,
};
pub(super) use crate::orchestrator::turn_outcome::TurnResult;
pub(super) use crate::protocol::contracts::{McpUsabilityState, UiMessageSnapshot};
pub(super) use agnt_kernel::bus::{LifecycleEvent, NarrationEvent, Subscription};
pub(super) use agnt_kernel::{SessionLifecycle, SessionRecord};

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

pub(super) struct CtxState {
    pub(super) worker_active: bool,
    pub(super) catalog: PickerCatalog,
    pending: Option<PendingPermissions>,
}

pub(super) struct HarnessParts<'a> {
    pub(super) stage: &'a mut UiRequestStage,
    pub(super) worker_tx: &'a mpsc::Sender<WorkerCommand>,
    pub(super) blocking_tx: &'a mpsc::Sender<UiRequestResponse>,
    pub(super) concurrent_tx: &'a mpsc::Sender<UiRequestResponse>,
    pub(super) bus: &'a Bus,
    pub(super) worker_rx: &'a mut Option<mpsc::Receiver<WorkerCommand>>,
    pub(super) narration_rx: &'a mut NarrationRx,
    pub(super) lifecycle_rx: &'a mut LifecycleRx,
    pub(super) stream_rx: &'a mut Subscription<SessionRecord>,
    pub(super) state: &'a mut CtxState,
}

pub(super) struct Harness {
    stage: UiRequestStage,
    worker_tx: mpsc::Sender<WorkerCommand>,
    worker_rx: Option<mpsc::Receiver<WorkerCommand>>,
    blocking_tx: mpsc::Sender<UiRequestResponse>,
    concurrent_tx: mpsc::Sender<UiRequestResponse>,
    bus: Bus,
    narration_rx: NarrationRx,
    lifecycle_rx: LifecycleRx,
    stream_rx: Subscription<SessionRecord>,
    state: CtxState,
}

impl Harness {
    pub(super) fn new() -> Self {
        let (worker_tx, worker_rx) = mpsc::channel::<WorkerCommand>(256);
        let (blocking_tx, _blocking_rx) = mpsc::channel::<UiRequestResponse>(256);
        let (concurrent_tx, _concurrent_rx) = mpsc::channel::<UiRequestResponse>(256);
        let bus = Bus::default();
        let narration_rx = bus.narration().subscribe();
        let lifecycle_rx = bus.lifecycle().subscribe();
        let stream_rx = bus.stream().subscribe();
        Self {
            stage: UiRequestStage::new(0),
            worker_tx,
            worker_rx: Some(worker_rx),
            blocking_tx,
            concurrent_tx,
            narration_rx,
            lifecycle_rx,
            stream_rx,
            bus,
            state: CtxState {
                worker_active: false,
                catalog: PickerCatalog::default(),
                pending: None,
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
            narration_rx: &mut self.narration_rx,
            lifecycle_rx: &mut self.lifecycle_rx,
            stream_rx: &mut self.stream_rx,
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

pub(super) fn take_warnings(narration_rx: &mut NarrationRx) -> Vec<String> {
    let mut out = Vec::new();
    while let Ok(event) = narration_rx.try_recv() {
        if let NarrationEvent::SystemMessage { text, .. } = event {
            out.push(text);
        }
    }
    out
}

/// Every stream record the stage published, in order — the replay a surface
/// folds after a session switch.
pub(super) fn take_records(stream_rx: &mut Subscription<SessionRecord>) -> Vec<SessionRecord> {
    let mut out = Vec::new();
    while let Ok(record) = stream_rx.try_recv() {
        out.push(record);
    }
    out
}

pub(super) fn take_lifecycle(lifecycle_rx: &mut LifecycleRx) -> Vec<LifecycleEvent> {
    let mut out = Vec::new();
    while let Ok(event) = lifecycle_rx.try_recv() {
        out.push(event);
    }
    out
}

pub(super) fn worker_rx_drop(h: &mut Harness) {
    h.worker_rx.take();
}
