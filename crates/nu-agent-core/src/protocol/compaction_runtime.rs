use crate::protocol::{
    compaction::{CompactionTriggerDecision, CompactionTriggerSource},
    contracts::ProgressUi,
};

/// Runtime capability for context compaction.
pub trait Compaction {
    fn evaluate_auto_compaction(&mut self) -> Option<CompactionTriggerDecision> {
        None
    }

    fn execute_compaction_trigger<U: ProgressUi + Send>(
        &mut self,
        _ui: &mut U,
        _source: CompactionTriggerSource,
    ) -> impl std::future::Future<Output = Result<(), String>> + Send {
        async move { Ok(()) }
    }
}
