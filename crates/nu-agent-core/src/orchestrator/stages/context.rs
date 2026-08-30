use tokio::sync::mpsc;

use nu_protocol::Span;

use crate::bus::Bus;
use crate::conversation::runtime::PendingPermissions;
use crate::orchestrator::{UiRequest, UiRequestResponse, WorkerCommand, turn_outcome::TurnOutcome};
use crate::protocol::event::PermissionDecisionSubmission;

/// Shared state passed to every stage on each poll iteration.
pub(crate) struct OrchestrationContext<'a> {
    /// Channel to send commands to the worker thread.
    pub worker_tx: &'a mpsc::Sender<WorkerCommand>,
    /// Channel for blocking UI request responses.
    pub blocking_response_tx: &'a mpsc::Sender<UiRequestResponse>,
    /// Channel for concurrent UI request responses.
    pub concurrent_response_tx: &'a mpsc::Sender<UiRequestResponse>,
    /// Pending permission map for the interactive permission resolver.
    pub pending: &'a Option<PendingPermissions>,
    /// Whether the worker thread is currently executing a turn.
    pub worker_active: &'a mut bool,
    /// Span used for `ExecuteTurn` commands.
    pub span: Span,
    /// External prompt that triggered the current turn (if any).
    /// Set by the main loop before dispatching a turn, consumed by the session
    /// stage when the turn completes.
    pub active_external_prompt: &'a mut Option<String>,
    /// Task ID of the external prompt that triggered the current turn (if any).
    /// Parallel to `active_external_prompt`; cleared by the session stage when
    /// the turn completes so a later cancel signal for the same task is ignored.
    pub active_external_task_id: &'a mut Option<String>,
    /// Task ID of an external cancel that arrived before the matching prompt was
    /// processed. Checked when an `ExternalPrompt` sets `active_external_task_id`.
    pub pending_external_cancel: &'a mut Option<String>,
    /// Shared signal bus. The session stage publishes turn-completion events.
    pub bus: &'a Bus,
}

/// Stage trait for slash command and prompt handling.
pub(crate) trait SlashHandler {
    async fn handle(&mut self, prompt: String, ctx: &mut OrchestrationContext);
}

/// Stage trait for permission decision handling.
pub(crate) trait PermissionHandler {
    fn handle(&mut self, decision: PermissionDecisionSubmission, ctx: &mut OrchestrationContext);
}

/// Stage trait for UI request handling.
pub(crate) trait UiRequestHandler {
    async fn handle_incoming(&mut self, request: UiRequest, ctx: &mut OrchestrationContext);
    async fn handle_blocking_response(
        &mut self,
        response: UiRequestResponse,
        ctx: &mut OrchestrationContext,
    );
    async fn handle_concurrent_response(
        &mut self,
        response: UiRequestResponse,
        ctx: &mut OrchestrationContext,
    );
    async fn drain_queued(&mut self, ctx: &mut OrchestrationContext);
    fn has_blocking_pending(&self) -> bool;
    fn has_pending(&self) -> bool;
}

/// Stage trait for session outcome handling.
pub(crate) trait SessionHandler {
    async fn handle_outcome(&mut self, outcome: TurnOutcome, ctx: &mut OrchestrationContext);
}
