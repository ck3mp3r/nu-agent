use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::compaction::CompactionStrategy;
use crate::conversation::managers::CompactionManager;
use crate::protocol::compaction::{
    CompactionTriggerDecision, CompactionTriggerPolicy, TokenCompactionPolicy,
};

pub struct CompactionState {
    context_window_max_tokens: u64,
    compaction_threshold_pct: f64,
    compaction_strategy: CompactionStrategy,
    compacting: Arc<AtomicBool>,
}

impl CompactionState {
    pub fn new(
        context_window_max_tokens: u64,
        compaction_threshold_pct: f64,
        compaction_strategy: CompactionStrategy,
    ) -> Self {
        Self {
            context_window_max_tokens,
            compaction_threshold_pct,
            compaction_strategy,
            compacting: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn compacting(&self) -> &Arc<AtomicBool> {
        &self.compacting
    }

    pub fn evaluate_auto_compaction(
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

impl CompactionManager for CompactionState {
    fn evaluate_auto_compaction(
        &mut self,
        last_total_tokens: Option<u64>,
    ) -> Option<CompactionTriggerDecision> {
        self.evaluate_auto_compaction(last_total_tokens)
    }

    fn compacting(&self) -> &Arc<AtomicBool> {
        &self.compacting
    }
}
