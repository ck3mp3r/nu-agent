use crate::protocol::{
    compaction::{CompactionTriggerDecision, CompactionTriggerSource},
    contracts::ProgressUi,
};

/// Runtime capability for context compaction.
pub trait HasCompaction {
    fn evaluate_auto_compaction(&mut self) -> Option<CompactionTriggerDecision> {
        None
    }

    fn execute_compaction_trigger<U: ProgressUi>(
        &mut self,
        _ui: &mut U,
        _source: CompactionTriggerSource,
    ) -> Result<(), String> {
        Ok(())
    }
}
