//! Conversation turn context types: `TurnConversation`, `TurnInput`, `TurnContext`.

use std::time::Duration;

use crate::config::Config;
use crate::conversation::compaction::CompactionConfig;
use crate::conversation::state::memory::MemoryOf;

use super::execute::CANCEL_GRACE;
use super::executor::ToolInfra;
use super::store::TurnMemory;

/// Context for executing a conversation turn.
pub struct TurnConversation<M: TurnMemory = MemoryOf> {
    /// The session's memory, opened for the turn's store and fact tables.
    pub memory: M,
    pub conversation_id: String,
    /// The persona that answered the turn, empty when none was loaded.
    ///
    /// The settled turn records it as its agent.
    pub agent: String,
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

impl<M: TurnMemory> TurnConversation<M> {
    /// The shared model slot, recovered if a switch panicked mid-swap.
    pub(crate) fn lock_dyn_model(
        &self,
    ) -> std::sync::MutexGuard<'_, rig::DynModel<rig::operation::Completion>> {
        self.dyn_model
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

pub struct TurnInput<'a> {
    pub prompt: String,
    pub preamble: Option<&'a str>,
    pub max_turns: Option<u32>,
}

pub struct TurnContext<'a, M: TurnMemory = MemoryOf> {
    pub(crate) conversation: TurnConversation<M>,
    pub(crate) input: TurnInput<'a>,
    pub(crate) tool_infra: ToolInfra,
    pub(crate) config: &'a Config,
    /// How long the turn keeps draining the stream after a bus cancel signal.
    /// Defaults to [`CANCEL_GRACE`]; tests set a short bound.
    pub(crate) cancel_grace: Duration,
}

impl<'a, M: TurnMemory> TurnContext<'a, M> {
    pub fn new(
        conversation: TurnConversation<M>,
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
