//! The channels and state one loop run owns.

use std::sync::Arc;

use tokio::sync::mpsc;

use crate::catalog::PickerCatalog;
use crate::orchestrator::events::UiRequestResponse;
use crate::orchestrator::external::{
    ExternalCompletion, ExternalTask, ExternalTaskStore, NoExternalTaskStore,
};
use crate::orchestrator::turn_outcome::TurnResult;
use agnt_kernel::bus::{ControlRx, LifecycleRx};

/// The receivers the loop owns for one run.
///
/// [`Loop::compose`](super::Loop) fills this from the config's intake and the
/// bus subscriptions, so no phase borrows a receiver from its caller.
pub(crate) struct LoopSources {
    /// Blocking UI request responses.
    pub(crate) blocking_response_rx: mpsc::Receiver<UiRequestResponse>,
    /// Concurrent UI request responses.
    pub(crate) concurrent_response_rx: mpsc::Receiver<UiRequestResponse>,
    /// The never-dropped command intake, read for the commands the loop owns.
    pub(crate) control_rx: ControlRx,
    /// The ordered lifecycle spine, read for compaction completion and for the
    /// prompts that arrive from outside the surface.
    pub(crate) lifecycle_rx: LifecycleRx,
    /// A2A task cancellation IDs.
    pub(crate) task_cancel_rx: Option<mpsc::UnboundedReceiver<String>>,
    /// Runtime-typed external tasks.
    pub(crate) external_task_rx: Option<mpsc::Receiver<ExternalTask>>,
    /// Runtime-typed external completions.
    pub(crate) external_completion_rx: Option<mpsc::Receiver<ExternalCompletion>>,
}

/// The channels the orchestrator loop selects over, in addition to the event
/// channel and the worker's outcome channel.
///
/// A transient borrow view over [`LoopSources`] and the worker's outcome
/// receiver, so the engine never takes ownership from the loop.
pub(crate) struct SourceChannels<'a, S: ExternalTaskStore = NoExternalTaskStore> {
    /// Worker turn outcomes.
    pub(crate) worker_result_rx: &'a mut mpsc::Receiver<TurnResult>,
    /// Blocking UI request responses.
    pub(crate) blocking_response_rx: &'a mut mpsc::Receiver<UiRequestResponse>,
    /// Concurrent UI request responses.
    pub(crate) concurrent_response_rx: &'a mut mpsc::Receiver<UiRequestResponse>,
    /// The never-dropped command intake, read for the commands the loop owns.
    pub(crate) control_rx: &'a mut ControlRx,
    /// The ordered lifecycle spine.
    pub(crate) lifecycle_rx: &'a mut LifecycleRx,
    /// A2A task cancellation IDs.
    pub(crate) task_cancel_rx: Option<mpsc::UnboundedReceiver<String>>,
    /// Runtime-typed external tasks.
    pub(crate) external_task_rx: Option<mpsc::Receiver<ExternalTask>>,
    /// Runtime-typed external completions.
    pub(crate) external_completion_rx: Option<mpsc::Receiver<ExternalCompletion>>,
    /// The store external tasks are rejected through, when one is configured.
    pub(crate) external_task_store: Option<&'a Arc<S>>,
}

/// The loop state a run owns and mutates across phases.
pub(crate) struct LoopContext {
    /// Channel for blocking UI request responses.
    pub(crate) blocking_response_tx: mpsc::Sender<UiRequestResponse>,
    /// Channel for concurrent UI request responses.
    pub(crate) concurrent_response_tx: mpsc::Sender<UiRequestResponse>,
    /// Whether the worker thread is currently executing a turn.
    pub(crate) worker_active: bool,
    /// The picker catalogs the ui-request stage answers a refresh from.
    ///
    /// `spawn_worker` builds the reader from the runtime before the worker
    /// takes it, so a picker fills while a turn runs.
    pub(crate) catalog: PickerCatalog,
    /// MCP-visible tool count the request stage seeds itself with.
    pub(crate) initial_visible_count: usize,
}

impl LoopContext {
    /// A context with no turn in flight.
    pub(crate) fn new(
        blocking_response_tx: mpsc::Sender<UiRequestResponse>,
        concurrent_response_tx: mpsc::Sender<UiRequestResponse>,
    ) -> Self {
        Self {
            blocking_response_tx,
            concurrent_response_tx,
            worker_active: false,
            catalog: PickerCatalog::default(),
            initial_visible_count: 0,
        }
    }
}
