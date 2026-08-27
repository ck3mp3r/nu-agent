pub mod compactor;

use crate::compaction::CompactionParams;
use crate::conversation::compaction::compactor::NoopProgressUi;
use crate::session::SessionStore;

/// The per-session machinery the hook needs to drive hook-driven compaction:
/// the standalone summarizer, the policy, and the token threshold. Does NOT
/// include the memory backend or session id (those are threaded separately
/// through the turn executor).
#[derive(Clone)]
pub struct CompactionConfig<S: SessionStore + Clone + Send + Sync> {
    pub compactor: compactor::NuCompactor<S, NoopProgressUi>,
    pub params: CompactionParams,
    pub threshold_tokens: Option<usize>,
}

#[cfg(test)]
mod compactor_test;
