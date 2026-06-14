use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::agent::protocol::compaction::{
    CompactionTriggerDecision, CompactionTriggerPolicy, TokenCompactionPolicy,
};
use crate::compaction::CompactionStrategy;

pub(crate) struct CompactionState {
    pub(crate) context_window_max_tokens: u64,
    pub(crate) compaction_threshold_pct: f64,
    pub(crate) compaction_count: usize,
    pub(crate) compaction_strategy: CompactionStrategy,
    pub(crate) compacting: Arc<AtomicBool>,
}

impl CompactionState {
    pub(crate) fn evaluate_auto_compaction(
        &mut self,
        last_total_tokens: Option<u64>,
    ) -> Option<CompactionTriggerDecision> {
        let policy = TokenCompactionPolicy::new(
            self.context_window_max_tokens,
            self.compaction_threshold_pct,
            self.compaction_strategy,
        );
        Some(policy.evaluate(last_total_tokens))
    }
}
