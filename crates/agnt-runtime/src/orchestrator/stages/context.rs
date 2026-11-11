use tokio::sync::mpsc;

use nu_protocol::Span;

use crate::bus::{ControlTx, LifecycleTx, NarrationTx, Stream};
use crate::catalog::PickerCatalog;
use crate::conversation::runtime::PendingPermissions;
use crate::orchestrator::events::{UiRequest, UiRequestResponse, WorkerCommand};
use crate::orchestrator::turn_outcome::TurnResult;
use crate::protocol::event::PermissionDecisionSubmission;
use agnt_kernel::bus::{CancelTx, FactsTx};

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
    /// The picker catalogs: the personas a surface offers and the sessions it
    /// can switch to.
    ///
    /// The reader answers a refresh from the handles the runtime holds, so a
    /// picker fills while a turn runs. The ui-request stage owns the one write it
    /// needs: a confirmed session switch moves the listing's omission.
    pub catalog: &'a mut PickerCatalog,
    /// Span used for `ExecuteTurn` commands.
    pub span: Span,
    /// The concerns the stages publish to, one hand per concern they write:
    /// the ordered record stream, the never-dropped control intake, the
    /// cancel signal, the ordered lifecycle spine, the narration text, and the
    /// latest-wins facts.
    pub stream: &'a Stream,
    pub control: &'a ControlTx,
    pub cancel: &'a CancelTx,
    pub lifecycle: &'a LifecycleTx,
    pub narration: &'a NarrationTx,
    /// The latest-wins kernel state snapshots the surfaces fold.
    pub facts: &'a FactsTx,
}

/// Stage trait for slash command and prompt handling.
pub(crate) trait SlashHandler {
    async fn handle(
        &mut self,
        prompt: String,
        task_id: Option<String>,
        ctx: &mut OrchestrationContext<'_>,
    ) {
        let _ = (prompt, task_id, ctx);
    }
}

/// Stage trait for permission decision handling.
pub(crate) trait PermissionHandler {
    async fn handle(
        &mut self,
        decision: PermissionDecisionSubmission,
        ctx: &mut OrchestrationContext<'_>,
    ) {
        let _ = (decision, ctx);
    }
}

/// Stage trait for UI request handling.
pub(crate) trait UiRequestHandler {
    async fn handle_incoming(&mut self, request: UiRequest, ctx: &mut OrchestrationContext<'_>) {
        let _ = (request, ctx);
    }
    async fn handle_blocking_response(
        &mut self,
        response: UiRequestResponse,
        ctx: &mut OrchestrationContext<'_>,
    ) {
        let _ = (response, ctx);
    }
    async fn handle_concurrent_response(
        &mut self,
        response: UiRequestResponse,
        ctx: &mut OrchestrationContext<'_>,
    ) {
        let _ = (response, ctx);
    }
    async fn drain_queued(&mut self, ctx: &mut OrchestrationContext<'_>) {
        let _ = ctx;
    }
    fn has_blocking_pending(&self) -> bool {
        false
    }
    fn has_pending(&self) -> bool {
        false
    }
}

/// Stage trait for session outcome handling.
pub(crate) trait SessionHandler {
    async fn handle_outcome(&mut self, outcome: TurnResult, ctx: &mut OrchestrationContext<'_>) {
        let _ = (outcome, ctx);
    }
}

/// A stage that implements every capability as a no-op.
///
/// Every trait method has a no-op default, so this impl is empty: a test that
/// exercises one capability fills the remaining `Stages` slots with `()` and
/// implements only the trait it proves.
impl SlashHandler for () {}
impl PermissionHandler for () {}
impl UiRequestHandler for () {}
impl SessionHandler for () {}
