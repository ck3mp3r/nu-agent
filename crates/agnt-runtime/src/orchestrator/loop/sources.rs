//! The channels and state one loop run owns.

use tokio::sync::mpsc;

use agnt_a2a::{A2aCompletionEvent, IncomingTask};

use crate::bus::CompactionRx;
use crate::orchestrator::events::UiRequestResponse;
use crate::orchestrator::turn_outcome::TurnOutcome;
use agnt_kernel::bus::ExternalRx;

/// The receivers the loop owns for one run.
///
/// [`Loop::compose`](super::Loop) fills this from the config's intake and the
/// bus subscriptions, so no phase borrows a receiver from its caller.
pub(crate) struct LoopSources {
    /// Blocking UI request responses.
    pub(crate) blocking_response_rx: mpsc::Receiver<UiRequestResponse>,
    /// Concurrent UI request responses.
    pub(crate) concurrent_response_rx: mpsc::Receiver<UiRequestResponse>,
    /// External (A2A) events from the bus.
    pub(crate) external_rx: ExternalRx,
    /// Compaction lifecycle events from the bus.
    pub(crate) compaction_rx: CompactionRx,
    /// A2A task cancellation IDs.
    pub(crate) task_cancel_rx: Option<mpsc::UnboundedReceiver<String>>,
    /// Incoming A2A tasks.
    pub(crate) a2a_task_rx: Option<mpsc::Receiver<IncomingTask>>,
    /// A2A completion events.
    pub(crate) a2a_completion_rx: Option<mpsc::Receiver<A2aCompletionEvent>>,
}

/// The channels the orchestrator loop selects over, in addition to the event
/// channel and the worker's outcome channel.
///
/// A transient borrow view over [`LoopSources`] and the worker's outcome
/// receiver, so the engine never takes ownership from the loop.
pub(crate) struct SourceChannels<'a> {
    /// Worker turn outcomes.
    pub(crate) worker_result_rx: &'a mut mpsc::Receiver<TurnOutcome>,
    /// Blocking UI request responses.
    pub(crate) blocking_response_rx: &'a mut mpsc::Receiver<UiRequestResponse>,
    /// Concurrent UI request responses.
    pub(crate) concurrent_response_rx: &'a mut mpsc::Receiver<UiRequestResponse>,
    /// External (A2A) events from the bus.
    pub(crate) external_rx: &'a mut ExternalRx,
    /// Compaction lifecycle events from the bus.
    pub(crate) compaction_rx: &'a mut CompactionRx,
    /// A2A task cancellation IDs.
    pub(crate) task_cancel_rx: Option<mpsc::UnboundedReceiver<String>>,
    /// Incoming A2A tasks.
    pub(crate) a2a_task_rx: Option<mpsc::Receiver<IncomingTask>>,
    /// A2A completion events.
    pub(crate) a2a_completion_rx: Option<mpsc::Receiver<A2aCompletionEvent>>,
}

/// The loop state a run owns and mutates across phases.
pub(crate) struct LoopContext {
    /// Channel for blocking UI request responses.
    pub(crate) blocking_response_tx: mpsc::Sender<UiRequestResponse>,
    /// Channel for concurrent UI request responses.
    pub(crate) concurrent_response_tx: mpsc::Sender<UiRequestResponse>,
    /// Whether the worker thread is currently executing a turn.
    pub(crate) worker_active: bool,
    /// External prompt that triggered the current turn (if any).
    pub(crate) active_external_prompt: Option<String>,
    /// Task ID of the external prompt that triggered the current turn (if any).
    pub(crate) active_external_task_id: Option<String>,
    /// Task ID of an external cancel that arrived before the matching prompt.
    pub(crate) pending_external_cancel: Option<String>,
    /// Task ID of an A2A task whose prompt is queued for the next turn.
    pub(crate) pending_a2a_task_id: Option<String>,
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
            active_external_prompt: None,
            active_external_task_id: None,
            pending_external_cancel: None,
            pending_a2a_task_id: None,
            initial_visible_count: 0,
        }
    }
}
