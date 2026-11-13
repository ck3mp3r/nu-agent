use rig::DynModel;
use rig::catalog::ModelSpec;
use rig::operation::Completion;
use rig_memory::CompactionPolicy;

use super::compactor::NuCompactor;

/// The per-session machinery the hook needs to drive hook-driven compaction:
/// the standalone summarizer, the policy the compactor plans with, and the
/// percent of the model's input limit a decision keeps free. Does NOT include
/// the memory backend or session id (those are threaded separately through the
/// turn executor).
#[derive(Clone)]
pub struct CompactionConfig {
    pub compactor: NuCompactor,
    /// Percent of the model's input limit kept free below the trigger. The
    /// decision derives the room from this and the turn's own spec.
    pub reserve_pct: u32,
}

impl CompactionConfig {
    /// The policy the compactor plans with.
    pub fn policy(&self) -> &CompactionPolicy {
        self.compactor.policy()
    }

    /// The room a decision leaves free below `spec`'s input limit, in tokens:
    /// `reserve_pct` percent of the limit, truncated.
    ///
    /// `None` when the model's input limit is unknown: nothing is over a
    /// threshold that no limit bounds, so the turn continues unpatched.
    pub fn keep_free(&self, spec: &ModelSpec) -> Option<u64> {
        let limit = u64::from(spec.input_limit()?);
        Some(limit.saturating_mul(u64::from(self.reserve_pct)) / 100)
    }

    /// The spec the compactor plans against, `None` when compaction is disabled
    /// for the session. It follows the active model, so it is the window of the
    /// model the session runs now.
    pub fn spec(&self) -> Option<ModelSpec> {
        self.compactor.spec()
    }

    /// The model the compactor summarizes with. It prices nothing: the hook
    /// prices a completion with the wire that served it.
    pub fn model(&self) -> DynModel<Completion> {
        self.compactor.model()
    }
}
