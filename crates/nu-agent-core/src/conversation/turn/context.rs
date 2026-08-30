//! Conversation turn context types: `TurnConversation`, `TurnInput`, `TurnContext`.

use crate::config::Config;
use crate::conversation::compaction::CompactionConfig;
use crate::conversation::state::memory::MemoryOf;
use crate::session::SessionStore;

use super::executor::ToolInfra;

/// Context for executing a conversation turn.
pub struct TurnConversation<S: SessionStore + Clone + Send + Sync> {
    pub memory: MemoryOf<S>,
    pub conversation_id: String,
    /// Whether this turn belongs to a persistent session.
    ///
    /// When `false` (transient), the rig `AgentBuilder` must NOT receive
    /// `.memory(...)` — rig would call `memory.append()` at turn end and write
    /// a `transient-{millis}.jsonl` file that is never reused and never cleaned
    /// up.  Omitting `.memory()` lets rig manage the turn's history in-memory
    /// within its own prompt call, which is exactly correct for a stateless
    /// one-shot invocation.
    pub has_session: bool,
    /// Shared runtime model handle. The agent is built from this handle and the
    /// hook's `on_model_select` routes each turn to its current value. It is
    /// constructed eagerly at startup.
    pub shared_model: std::sync::Arc<std::sync::Mutex<rig::agent::ModelHandle>>,
    /// Hook-driven compaction machinery: compactor, policy, force flag, threshold.
    pub compaction: CompactionConfig<S>,
}

pub struct TurnInput<'a> {
    pub prompt: String,
    pub preamble: Option<&'a str>,
    pub max_turns: Option<u32>,
}

pub struct TurnContext<'a, S>
where
    S: SessionStore + Clone + Send + Sync,
{
    pub(crate) conversation: TurnConversation<S>,
    pub(crate) input: TurnInput<'a>,
    pub(crate) tool_infra: ToolInfra,
    pub(crate) config: &'a Config,
}

impl<'a, S> TurnContext<'a, S>
where
    S: SessionStore + Clone + Send + Sync,
{
    pub fn new(
        conversation: TurnConversation<S>,
        input: TurnInput<'a>,
        tool_infra: ToolInfra,
        config: &'a Config,
    ) -> Self {
        Self {
            conversation,
            input,
            tool_infra,
            config,
        }
    }
}
