use std::sync::mpsc;

use crate::orchestrator::stages::{OrchestrationContext, StageOutcome};
use crate::orchestrator::{
    PendingAutoCompaction, PendingCompactionTrigger, WorkerCommand, poll_option_channel,
};
use crate::protocol::contracts::{
    DisplayStateUi, LifecycleUi, ProgressUi, TranscriptUi, UserInputUi,
};
use crate::protocol::event::UiEvent;

/// Polls auto-compaction evaluation and manual compaction trigger responses.
pub(crate) struct CompactionStage {
    pending_auto_compaction: Option<PendingAutoCompaction>,
    pending_compaction_trigger: Option<PendingCompactionTrigger>,
}

impl CompactionStage {
    pub fn new() -> Self {
        Self {
            pending_auto_compaction: None,
            pending_compaction_trigger: None,
        }
    }

    pub fn poll<U>(&mut self, ctx: &mut OrchestrationContext<'_, U>) -> StageOutcome
    where
        U: ProgressUi + UserInputUi + DisplayStateUi + LifecycleUi + TranscriptUi,
    {
        if *ctx.worker_active {
            self.pending_auto_compaction = None;
            self.pending_compaction_trigger = None;
            *ctx.should_evaluate_compaction = false;
            return StageOutcome::Idle;
        }

        let mut handled = false;

        if let Some(response_rx) = self.pending_compaction_trigger.take() {
            let (message, rx) = poll_option_channel(response_rx);
            if let Some(msg) = message {
                ctx.ui.emit(&UiEvent::Warning { message: msg });
                handled = true;
            } else if let Some(rx) = rx {
                self.pending_compaction_trigger = Some(rx);
            } else {
                ctx.ui.emit(&UiEvent::Warning {
                    message: "Compaction worker disconnected".to_string(),
                });
                handled = true;
            }
        }

        if *ctx.should_evaluate_compaction && self.pending_auto_compaction.is_none() {
            *ctx.should_evaluate_compaction = false;
            let (response_tx, response_rx) = mpsc::channel();
            if ctx
                .worker_tx
                .send(WorkerCommand::EvaluateAutoCompaction { response_tx })
                .is_ok()
            {
                self.pending_auto_compaction = Some(response_rx);
                handled = true;
            }
        }

        if let Some(response_rx) = self.pending_auto_compaction.take() {
            let (message, rx) = poll_option_channel(response_rx);
            if let Some(msg) = message {
                ctx.ui.emit(&UiEvent::Warning { message: msg });
                handled = true;
            } else if let Some(rx) = rx {
                self.pending_auto_compaction = Some(rx);
            }
            // Disconnected: silently drop (no warning needed for auto-compaction disconnect)
        }

        if handled {
            StageOutcome::Handled
        } else {
            StageOutcome::Idle
        }
    }

    /// Called by `SlashStage` (via `OrchestratorStages::poll_all`) when a `/compact`
    /// command is submitted. Stores the pending trigger receiver so it can be polled
    /// on the next iteration.
    pub fn set_pending_compaction_trigger(&mut self, rx: PendingCompactionTrigger) {
        self.pending_compaction_trigger = Some(rx);
    }

    /// Returns `true` if any compaction work is in flight.
    pub fn has_pending(&self) -> bool {
        self.pending_auto_compaction.is_some() || self.pending_compaction_trigger.is_some()
    }
}
