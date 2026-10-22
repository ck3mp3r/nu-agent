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

    /// The model the compactor summarizes with — the same wire the agent
    /// calls, so its catalog-bound spec prices a completion.
    pub fn model(&self) -> DynModel<Completion> {
        self.compactor.model()
    }
}
