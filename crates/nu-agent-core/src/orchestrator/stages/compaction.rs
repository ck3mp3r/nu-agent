use crate::bus::WarningEvent;
use crate::orchestrator::stages::{CompactionHandler, OrchestrationContext};

/// Handles auto-compaction evaluation and compaction result routing.
pub(crate) struct CompactionStage {
    has_auto_compaction_pending: bool,
}

impl CompactionStage {
    pub fn new() -> Self {
        Self {
            has_auto_compaction_pending: false,
        }
    }
}

impl CompactionHandler for CompactionStage {
    fn handle_result(&mut self, message: Option<String>, ctx: &mut OrchestrationContext) {
        self.has_auto_compaction_pending = false;
        if let Some(msg) = message {
            let _ = ctx
                .bus
                .warning()
                .send(WarningEvent::Message { message: msg });
        }
    }

    fn set_pending_auto_compaction(&mut self) {
        self.has_auto_compaction_pending = true;
    }

    fn has_pending_auto_compaction(&self) -> bool {
        self.has_auto_compaction_pending
    }

    fn has_pending(&self) -> bool {
        self.has_auto_compaction_pending
    }
}
