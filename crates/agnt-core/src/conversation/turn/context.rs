//! Conversation turn context types: `TurnConversation`, `TurnInput`, `TurnContext`.

use std::time::Duration;

use crate::config::Config;
use crate::conversation::compaction::CompactionConfig;
use crate::conversation::state::memory::MemoryOf;

use super::execute::CANCEL_GRACE;
use super::executor::ToolInfra;

/// Context for executing a conversation turn.
pub struct TurnConversation {
    pub memory: MemoryOf,
    pub conversation_id: String,
    /// Whether this turn belongs to a persistent session.
    ///
    /// When `false` (transient), the turn's message delta is not persisted: no
    /// session log is written for a stateless one-shot invocation. When `true`,
    /// the agent writes the run's delta to the session's rig-memory store.
    pub has_session: bool,
    /// Shared runtime model. The agent is built from this model on every turn
    /// and `switch_model()` swaps the inner value. It is constructed eagerly at
    /// startup.
    pub dyn_model: std::sync::Arc<std::sync::Mutex<rig::DynModel<rig::operation::Completion>>>,
    /// Hook-driven compaction machinery: compactor, policy, force flag, threshold.
    pub compaction: CompactionConfig,
}

pub struct TurnInput<'a> {
    pub prompt: String,
    pub preamble: Option<&'a str>,
    pub max_turns: Option<u32>,
}

pub struct TurnContext<'a> {
    pub(crate) conversation: TurnConversation,
    pub(crate) input: TurnInput<'a>,
    pub(crate) tool_infra: ToolInfra,
    pub(crate) config: &'a Config,
    /// How long the turn keeps draining the stream after a bus cancel signal.
    /// Defaults to [`CANCEL_GRACE`]; tests set a short bound.
    pub(crate) cancel_grace: Duration,
}

impl<'a> TurnContext<'a> {
    pub fn new(
        conversation: TurnConversation,
        input: TurnInput<'a>,
        tool_infra: ToolInfra,
        config: &'a Config,
    ) -> Self {
        Self {
            conversation,
            input,
            tool_infra,
            config,
            cancel_grace: CANCEL_GRACE,
        }
    }

    /// Set how long the turn keeps draining the stream after a bus cancel
    /// signal. Tests pass a short bound so a stalled stream returns quickly.
    pub fn with_cancel_grace(mut self, cancel_grace: Duration) -> Self {
        self.cancel_grace = cancel_grace;
        self
    }
}
