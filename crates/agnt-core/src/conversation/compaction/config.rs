use crate::compaction::CompactionParams;

use rig::DynModel;
use rig::catalog::ModelSpec;
use rig::operation::Completion;
use rig_memory::CompactionPolicy;

use super::compactor::NuCompactor;

/// The per-session machinery the hook needs to drive hook-driven compaction:
/// the standalone summarizer, the policy, and the token threshold. Does NOT
/// include the memory backend or session id (those are threaded separately
/// through the turn executor).
#[derive(Clone)]
pub struct CompactionConfig {
    pub compactor: NuCompactor,
    pub params: CompactionParams,
    pub threshold_tokens: Option<usize>,
    /// The fraction of the active model's context the compaction threshold
    /// uses. A model switch re-derives `threshold_tokens` from it, so the
    /// build-time threshold and any refresh can never diverge.
    pub threshold_pct: f64,
}

impl CompactionConfig {
    /// The policy the compactor plans with.
    pub fn policy(&self) -> &CompactionPolicy {
        self.compactor.policy()
    }

    /// The spec the compactor plans against, `None` when compaction is disabled
    /// for the session.
    pub fn spec(&self) -> Option<&ModelSpec> {
        self.compactor.spec()
    }

    /// The model the compactor summarizes with. It prices nothing: the hook
    /// prices a completion with the wire that served it.
    pub fn model(&self) -> DynModel<Completion> {
        self.compactor.model()
    }
}
