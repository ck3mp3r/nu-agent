/// Manager traits for the decomposed `AgentRuntime` components.
///
/// Each trait captures the minimal interface that `AgentRuntime` method bodies
/// actually call — no phantom methods, no speculative surface area.
///
/// All traits use static dispatch (generics with trait bounds) per AGENTS.md:
/// no `Box<dyn Trait>`, no `&dyn Trait`.
use nu_protocol::LabeledError;

use crate::config::{Config, PluginConfig};
use crate::conversation::providers::CachedProviderClient;
use crate::protocol::compaction::CompactionTriggerDecision;
use crate::session::JournalConversationMemory;
use crate::tools::authz::PermissionsConfig;
use crate::tools::closure::ClosureRegistry;
use crate::tools::handler::McpToolRegistry;
use crate::types::ToolDefinition;

// ── ProviderManager ──────────────────────────────────────────────────────────

/// Manages LLM provider client lifecycle: caching, model switching, config access.
pub trait ProviderManager {
    /// Return the current runtime config.
    fn provider_config(&self) -> &Config;

    /// Return a mutable reference to the current runtime config.
    fn provider_config_mut(&mut self) -> &mut Config;

    /// Ensure the cached client is up to date for the current config.
    /// Idempotent when the key is unchanged.
    fn ensure_client_cached(&mut self) -> Result<(), LabeledError>;

    /// Return a reference to the cached client, if one exists.
    fn cached_client(&self) -> Option<&CachedProviderClient>;

    /// Discard the cached client so the next `ensure_client_cached` rebuilds it.
    fn invalidate_cache(&mut self);

    /// Switch to a different model spec, updating config and invalidating cache.
    /// Returns the new `"provider/model"` identity on success.
    fn switch_model(&mut self, model_spec: &str) -> Result<String, String>;

    /// Return the startup plugin config snapshot (used for model-switch resolution).
    fn startup_plugin_config(&self) -> Option<&PluginConfig>;
}

// ── ToolManager ─────────────────────────────────────────────────────────────

/// Manages tool definitions and the closure registry.
pub trait ToolManager {
    /// Return a reference to the closure registry.
    fn closure_registry(&self) -> &ClosureRegistry;

    /// Return a mutable reference to the raw tool definitions vector.
    fn tool_definitions_mut(&mut self) -> &mut Vec<ToolDefinition>;

    /// Revert tool definitions to the baseline set captured at startup.
    fn reset_to_baseline(&mut self);

    /// Return the LLM-visible tool definitions filtered by MCP registry and permissions.
    fn active_definitions(
        &self,
        mcp_registry: &McpToolRegistry,
        permissions: &PermissionsConfig,
    ) -> Vec<ToolDefinition>;
}

// ── SessionManager ───────────────────────────────────────────────────────────

/// Manages conversation memory and session IDs.
pub trait SessionManager {
    /// Return a shared reference to the underlying `JournalConversationMemory`.
    fn memory(&self) -> &JournalConversationMemory;

    /// Return a mutable reference to the underlying `JournalConversationMemory`.
    fn memory_mut(&mut self) -> &mut JournalConversationMemory;

    /// Clear all messages from conversation memory and reset token tracking.
    fn clear(&mut self);

    /// Return the last recorded total-token count (for compaction evaluation).
    fn last_total_tokens(&self) -> Option<u64>;

    /// Return a mutable reference to the last total token counter.
    fn last_total_tokens_mut(&mut self) -> &mut Option<u64>;
}

// ── CompactionManager ────────────────────────────────────────────────────────

/// Manages context compaction threshold evaluation and the in-flight guard flag.
pub trait CompactionManager {
    /// Evaluate whether auto-compaction should fire given the last token count.
    fn evaluate_auto_compaction(
        &mut self,
        last_total_tokens: Option<u64>,
    ) -> Option<CompactionTriggerDecision>;

    /// Return a reference to the in-flight compaction guard.
    /// Used by `AgentRuntime` to prevent concurrent compaction runs.
    fn compacting(&self) -> &std::sync::Arc<std::sync::atomic::AtomicBool>;
}

// ── PersonaManager ───────────────────────────────────────────────────────────

/// Manages agent persona: identity, preamble fragments, agent switching.
pub trait PersonaManager {
    /// Return the raw agent persona body text (system prompt override).
    fn agent_persona_body(&self) -> Option<&str>;

    /// Return the cached sub-agent instruction fragment.
    fn cached_sub_agent_instruction(&self) -> Option<&str>;

    /// Return the cached agents-chain preamble fragment.
    fn cached_agents_chain(&self) -> Option<&str>;

    /// Return the cached available-skills preamble fragment.
    fn cached_available_skills(&self) -> Option<&str>;

    /// Return the active agent identity string.
    fn agent_identity(&self) -> Option<&str>;

    /// Return the length of the agent persona body in bytes, if present.
    fn persona_body_len(&self) -> Option<usize>;

    /// Return the agent description string, if present.
    fn agent_description(&self) -> Option<&str>;

    /// Return the formatted `"provider/model"` identity using `config`.
    fn active_model_identity(&self, config: &Config) -> String;

    /// Switch to the named agent, reading its persona file from disk.
    /// Returns a `SwitchAgentResult` carrying the new identity and optional model.
    fn switch_agent(
        &mut self,
        agent_name: &str,
        cwd: &std::path::Path,
        agents_config: &crate::config::AgentsConfig,
    ) -> Result<crate::conversation::state::persona::SwitchAgentResult, String>;
}

// ── MultiAgentManager ────────────────────────────────────────────────────────

/// Manages multi-agent coordination: mailbox, agent registry.
pub trait MultiAgentManager {
    /// Return the available agent summaries for preamble injection.
    fn available_agent_summaries(&self) -> &[crate::protocol::persona::PersonaSummary];

    /// Return the agents configuration.
    fn agents_config(&self) -> &crate::config::AgentsConfig;

    /// Take ownership of the mailbox receiver (drains it from `self`).
    fn take_mailbox_rx(
        &mut self,
    ) -> Option<std::sync::mpsc::Receiver<crate::mailbox::IncomingMessage>>;
}
