use std::sync::mpsc as std_mpsc;
use tokio::sync::mpsc;

use nu_protocol::{LabeledError, Span};

use crate::conversation::runtime::PendingPermissions;
use crate::orchestrator::WorkerCommand;
use crate::orchestrator::turn_outcome::TurnOutcome;
use crate::protocol::contracts::{
    DisplayStateUi, LifecycleUi, ProgressUi, TranscriptUi, UserInputUi,
};

pub mod compaction;
pub mod model;
pub mod permission;
pub mod session;
pub mod slash;

use compaction::CompactionStage;
use model::ModelSwitchStage;
use permission::PermissionStage;
use session::SessionStage;
use slash::SlashStage;

/// The outcome of polling a single stage.
#[derive(Debug, Clone)]
pub(crate) enum StageOutcome {
    /// Nothing consumed — no event was ready.
    Idle,
    /// An event was consumed; the main loop should continue immediately.
    Handled,
    /// The worker channel is closed; the loop must terminate with this error.
    Fatal(LabeledError),
}

/// Shared state passed to every stage on each poll iteration.
///
/// `U` is a struct-level generic because this is a short-lived borrow
/// scoped to a single loop iteration. The stage structs themselves have **no**
/// type parameters.
pub(crate) struct OrchestrationContext<'a, U> {
    /// Channel to send commands to the worker thread.
    pub worker_tx: &'a mpsc::Sender<WorkerCommand>,
    /// Pending permission map for the interactive permission resolver.
    pub pending: &'a Option<PendingPermissions>,
    /// Whether the worker thread is currently executing a turn.
    pub worker_active: &'a mut bool,
    /// Re-arm flag: set to `true` after a turn completes so compaction is re-evaluated.
    pub should_evaluate_compaction: &'a mut bool,
    /// Span used for `ExecuteTurn` commands.
    pub span: Span,
    /// The interactive UI. Stages call `ui.emit(...)`, `ui.take_submitted_prompt()`, etc.
    pub ui: &'a mut U,
    /// External prompt that triggered the current turn (if any).
    /// Set by the main loop before dispatching a turn, consumed by the session
    /// stage when the turn completes so it can fire `on_turn_complete`.
    pub active_external_prompt: &'a mut Option<String>,
    /// Task ID of the external prompt that triggered the current turn (if any).
    /// Parallel to `active_external_prompt`; cleared by the session stage when
    /// the turn completes so a later cancel signal for the same task is ignored.
    pub active_external_task_id: &'a mut Option<String>,
    /// Optional sender for turn-completion notifications. When Some, the session
    /// stage fires `(prompt_text, response_text)` after each turn that was
    /// triggered by an external prompt.
    pub on_turn_complete: &'a Option<std_mpsc::Sender<(String, String)>>,
}

/// All orchestration stages, bundled as a single composable unit.
///
/// This struct has **no** type parameters. `U` appears only on `poll_all`
/// as a method-level generic and is inferred at every call site.
pub(crate) struct OrchestratorStages {
    permission: PermissionStage,
    compaction: CompactionStage,
    model: ModelSwitchStage,
    session: SessionStage,
    slash: SlashStage,
}

impl OrchestratorStages {
    /// Create all stages.
    ///
    /// - `initial_visible_count`: authoritative MCP tool count from startup, used by
    ///   `ModelSwitchStage` to report accurate counts on failure before any toggle succeeds.
    /// - `worker_result_rx`: receives `TurnOutcome` values from the worker thread.
    pub fn new(
        initial_visible_count: usize,
        worker_result_rx: mpsc::Receiver<TurnOutcome>,
    ) -> Self {
        Self {
            permission: PermissionStage::new(),
            compaction: CompactionStage::new(),
            model: ModelSwitchStage::new(initial_visible_count),
            session: SessionStage::new(worker_result_rx),
            slash: SlashStage::new(),
        }
    }

    /// Poll all stages in order. Returns `Handled` on the first stage that
    /// consumed an event; returns `Idle` if all stages were idle.
    /// Returns `Fatal(e)` immediately if any stage signals a fatal error —
    /// no further stages are polled.
    ///
    /// Stage execution order preserves the original loop semantics:
    /// - `session` first: updates `worker_active` so downstream stages see the current flag
    /// - `model` second: drains queued switches now that `worker_active` is updated
    /// - `compaction`, `permission`, `slash` in original order
    ///
    /// `slash` is gated on `!model.has_pending_model_switch()` to match the original
    /// guard `if !worker_active && pending_model_switch.is_none()`.
    pub async fn poll_all<U>(&mut self, ctx: &mut OrchestrationContext<'_, U>) -> StageOutcome
    where
        U: ProgressUi + UserInputUi + DisplayStateUi + LifecycleUi + TranscriptUi,
    {
        // session first: updates worker_active so downstream stages see the correct flag
        let session = self.session.poll(ctx);
        if let StageOutcome::Fatal(_) = session {
            return session;
        }
        // model: polls results and drains queued switches (worker_active now current)
        let model = self.model.poll(ctx).await;
        if let StageOutcome::Fatal(_) = model {
            return model;
        }
        let compaction = self.compaction.poll(ctx).await;
        if let StageOutcome::Fatal(_) = compaction {
            return compaction;
        }
        let permission = self.permission.poll(ctx);
        if let StageOutcome::Fatal(_) = permission {
            return permission;
        }
        // slash: guard on no pending model switch (preserves original loop semantics)
        let slash = if !self.model.has_pending_model_switch() {
            self.slash.poll(ctx).await
        } else {
            StageOutcome::Idle
        };
        if let StageOutcome::Fatal(_) = slash {
            return slash;
        }

        // Cross-stage handoff: if slash dispatched a /compact, give the trigger
        // receiver to CompactionStage so it can poll the result next iteration.
        if let Some(rx) = self.slash.take_pending_compaction_trigger() {
            self.compaction.set_pending_compaction_trigger(rx);
        }

        [session, model, compaction, permission, slash]
            .into_iter()
            .find(|o| matches!(o, StageOutcome::Handled))
            .unwrap_or(StageOutcome::Idle)
    }

    /// Returns `true` if any async operations are in flight across all stages.
    /// Used by the main loop's quit check to decide whether to defer shutdown.
    pub fn has_pending_ops(&self) -> bool {
        self.model.has_pending() || self.compaction.has_pending()
    }
}
