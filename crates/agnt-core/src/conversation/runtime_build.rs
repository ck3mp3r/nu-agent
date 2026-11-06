use std::sync::{Arc, Mutex};

use nu_protocol::LabeledError;

use crate::bus::Bus;
use crate::conversation::runtime::AgentConversationRuntime;
use crate::conversation::runtime_parts::RuntimeParts;
use crate::hooks::HookAgentContext;
use crate::protocol::persona::PersonaSummary;
use crate::types::ToolDefinition;
use agnt_runtime::config::{
    AgentsConfig, CompactionConfig, Config, ModelRoleConfig, PluginConfig, vault::Vault,
};
use agnt_runtime::tools::authz::{PermissionsConfig, PermissionsOverlay};
use agnt_runtime::tools::closure::{ClosureRegistry, EngineInterfaceLike};
use agnt_runtime::tools::handler::McpToolRegistry;
use agnt_runtime::tools::mcp::config::McpServerConfig;
use agnt_runtime::tools::mcp::runtime::{McpRuntime, McpServerLifecycle};
use rig::DynModel;
use rig::operation::Completion;
use rig::tool::server::ToolServerHandle;

pub struct RuntimeBuildParams {
    pub runtime: tokio::runtime::Handle,
    pub config: Config,
    pub plugin_config: Option<PluginConfig>,
    pub tool_definitions: Vec<ToolDefinition>,
    pub baseline_tool_definitions: Vec<ToolDefinition>,
    pub closure_registry: ClosureRegistry,
    pub mcp_runtime: Option<McpRuntime>,
    pub tool_server_handle: ToolServerHandle,
    pub mcp_lifecycle_projection: Vec<McpServerLifecycle>,
    pub mcp_server_configs: Vec<McpServerConfig>,
    pub mcp_caller_cwd: Option<std::path::PathBuf>,
    pub vault: Option<Arc<Vault>>,
    pub mcp_registry: McpToolRegistry,
    pub engine: Arc<dyn EngineInterfaceLike>,
    /// The rig-memory store config the session's store is made from.
    pub rig: crate::session::RigStoreConfig,
    pub final_session_id: Option<String>,
    pub proactive_threshold_pct: Option<f64>,
    pub base_permissions: PermissionsConfig,
    pub effective_permissions: PermissionsConfig,
    pub cli_permissions_overlay: Option<PermissionsOverlay>,
    pub permissions_startup_summary: String,
    pub persona_body: Option<String>,
    pub agent_identity: Option<String>,
    pub agent_description: Option<String>,
    pub agent_icon: Option<String>,
    pub cached_agents_chain: Option<String>,
    pub cached_available_skills: Option<String>,
    pub available_agents: Vec<PersonaSummary>,
    pub agents_config: AgentsConfig,
    pub cwd: std::path::PathBuf,
    /// The resolved application paths.
    ///
    /// Carried so the runtime's agent switch and the hook runners never re-derive
    /// a config or persona directory.
    pub paths: agnt_runtime::config::Paths,
    pub bus: Bus,
    /// The agent snapshot every hook envelope is stamped with.
    ///
    /// The caller creates it and clones it into the hook runners. The runtime
    /// writes the model identity here on construction and refreshes both cells
    /// on every persona or model switch.
    pub hook_context: HookAgentContext,
    /// The compactor's erased model, when the composition root builds it itself.
    ///
    /// `None` derives it: the `[models.light]` role when it resolves, else the
    /// startup active model. A caller that supplies one builds it from the
    /// active model, so the compaction identity and its planning spec are the
    /// active model's.
    pub compactor_model: Option<Arc<Mutex<DynModel<Completion>>>>,
}

pub async fn build_runtime(
    params: RuntimeBuildParams,
) -> Result<AgentConversationRuntime, LabeledError> {
    use crate::conversation::state::{
        memory::MemoryState, multi_agent::MultiAgentState, persona::PersonaState,
        provider::ProviderState, tool::ToolState,
    };

    // The parts own their state; the record below keeps the values `compose`
    // derives from. Every copy is taken once, at startup.
    let parts = RuntimeParts {
        provider: ProviderState::new(params.config.clone(), params.plugin_config.clone())
            .with_copilot_token_dir(params.paths.copilot_token_dir.clone()),
        tools: ToolState::new(
            params.tool_definitions.clone(),
            params.baseline_tool_definitions.clone(),
            params.closure_registry.clone(),
        ),
        session: MemoryState::new(params.rig.clone()),
        persona: PersonaState::new(
            params.persona_body.clone(),
            params.agent_identity.clone(),
            params.agent_description.clone(),
            params.agent_icon.clone(),
            params.cached_agents_chain.clone(),
            params.cached_available_skills.clone(),
        ),
        multi: MultiAgentState::new(
            params.available_agents.clone(),
            params.agents_config.clone(),
        ),
        engine: Arc::clone(&params.engine),
        bus: params.bus.clone(),
    };

    AgentConversationRuntime::compose(parts, params).await
}

/// The config the compaction (summarizer) model is built from: the
/// `[models.light]` role when it resolves, else `None` so the caller falls
/// back to the startup active model.
///
/// Every fallback names itself in the debug log, so a session that summarizes
/// with the active model is explainable.
pub(crate) fn resolve_light_config(plugin_config: Option<&PluginConfig>) -> Option<Config> {
    let Some(plugin_config) = plugin_config else {
        log::debug!("compaction model: no plugin config; using the startup active model");
        return None;
    };
    let Some(role) = plugin_config.models.get("light") else {
        log::debug!("compaction model: no [models.light] role; using the startup active model");
        return None;
    };
    match plugin_config.resolve_model(role) {
        Ok(config) => Some(config),
        Err(error) => {
            log::debug!(
                "compaction model: [models.light] did not resolve ({error}); using the startup active model"
            );
            None
        }
    }
}

/// The compaction policy a `[compaction]` block asks for.
///
/// Starts from rig-memory's default policy and overwrites each field the block
/// sets. A field the block leaves unset keeps rig-memory's default value. rig
/// owns the meaning of every value, so no range check runs here; `near_window`
/// has no config key and always keeps rig-memory's default.
pub fn build_compaction_policy(cfg: &CompactionConfig) -> rig_memory::CompactionPolicy {
    let mut policy = rig_memory::CompactionPolicy::default();

    if let Some(reserve) = cfg.reserve {
        policy.summarizer.limits.reserve = reserve;
    }
    if let Some(summary_tokens) = cfg.summary_tokens {
        policy.summarizer.limits.summary_tokens = summary_tokens;
    }
    if let Some(snippet_chars) = cfg.snippet_chars {
        policy.summarizer.limits.snippet_chars = snippet_chars;
    }
    if let Some(unknown_window_chars) = cfg.unknown_window_chars {
        policy.summarizer.limits.unknown_window_chars = unknown_window_chars;
    }
    if let Some(keep_recent) = cfg.keep_recent {
        policy.keep_recent = keep_recent;
    }
    if let Some(keep_asked) = cfg.keep_asked {
        policy.keep_asked = keep_asked;
    }
    if let Some(clearing) = &cfg.clearing {
        policy.clearing = clearing.clone();
    }
    if let Some(tracked) = &cfg.tracked {
        policy.tracked = tracked.clone();
    }
    if let Some(prompts) = &cfg.prompts {
        policy.summarizer.prompts = prompts.clone();
    }

    policy
}

/// Apply persona model override if CLI --model was not provided.
///
/// `persona_model` can be:
/// - A literal `provider/model` string (contains `/`) → used as-is
/// - A role label (no `/`) → resolved through `plugin_config.models`
///
/// Returns `Ok(true)` if persona model was applied, `Ok(false)` if skipped.
/// Returns `Err` if a role label is unknown or not configured.
pub fn apply_persona_model(
    config: &mut Config,
    plugin_config: Option<&PluginConfig>,
    persona_model: Option<&str>,
    cli_model_provided: bool,
) -> Result<bool, LabeledError> {
    if cli_model_provided {
        return Ok(false);
    }
    let Some(m) = persona_model else {
        return Ok(false);
    };

    let role_config = if m.contains('/') {
        // Literal provider/model string — use as-is with default overrides
        ModelRoleConfig {
            model: m.to_string(),
            ..Default::default()
        }
    } else {
        // Role label — resolve through plugin_config.models
        let pc = plugin_config.ok_or_else(|| {
            LabeledError::new(format!(
                "Unknown model role: '{m}'. No plugin config available — role labels require a plugin config with a models map."
            ))
        })?;
        pc.models
            .get(m)
            .ok_or_else(|| {
                let mut available: Vec<&str> = pc.models.keys().map(|s| s.as_str()).collect();
                available.sort();
                LabeledError::new(format!(
                    "Unknown model role: '{m}'. Available roles: {}",
                    available.join(", ")
                ))
            })?
            .clone()
    };

    // Re-resolve the full config using resolve_model with the role config.
    // This replaces provider, model, provider_impl, and all role-level overrides.
    let pc = plugin_config
        .ok_or_else(|| LabeledError::new("Plugin config required for persona model resolution"))?;
    *config = pc
        .resolve_model(&role_config)
        .map_err(|msg| LabeledError::new(format!("Failed to resolve persona model: {msg}")))?;

    // Re-resolve preamble for the new model.
    if let Some((provider_name, model_name)) = role_config.model.split_once('/') {
        config.preamble =
            agnt_runtime::config::resolve_preamble_for_model(pc, provider_name, model_name);
    }

    log::debug!(
        "apply_persona_model: overriding to provider={}, model={}",
        config.provider,
        config.model
    );
    Ok(true)
}

/// Apply per-persona config overrides to runtime Config.
///
/// Persona front matter sits between CLI flags and plugin/env/defaults in the precedence chain:
///   CLI flags > persona front matter > plugin config / env / built-in defaults
///
/// `cli_max_turns_provided`: true when the user explicitly passed the max-turns CLI flag.
/// This is needed because `max_tool_turns` may already be `Some(20)` from the pipeline-mode
/// default (not a CLI flag) — persona must override that default.
pub fn apply_persona_config(
    config: &mut Config,
    persona: &crate::protocol::persona::ParsedPersona,
    cli_max_turns_provided: bool,
) {
    if config.temperature.is_none() {
        config.temperature = persona.temperature;
    }
    if config.max_tokens.is_none() {
        config.max_tokens = persona.max_tokens;
    }
    // max_tool_turns: only skip if the CLI explicitly provided the flag.
    // The pipeline-mode default (Some(20)) must be overridable by persona.
    if !cli_max_turns_provided && let Some(t) = persona.max_tool_turns {
        config.max_tool_turns = Some(t);
    }
    if config.max_tool_calls_per_subturn.is_none() {
        config.max_tool_calls_per_subturn = persona.max_tool_calls_per_subturn;
    }
    if config.max_tool_result_bytes.is_none() {
        config.max_tool_result_bytes = persona.max_tool_result_bytes;
    }
    if config.additional_params.is_none() {
        config.additional_params = persona.additional_params.clone();
    }
}

/// Cache preamble components once at startup — loaded once, reused every turn.
/// Returns (cached_agents_chain, cached_available_skills).
///
/// The user-level `AGENTS.md` comes from `agents_dir`, the resolved persona
/// directory; the cwd and `HOME` candidates are unchanged.
pub fn build_preamble_cache(
    cwd: &std::path::Path,
    agents_dir: &std::path::Path,
) -> (Option<String>, Option<String>) {
    let loaded_agents_result =
        crate::protocol::agents::load_agents_chain_for_cwd(cwd, Some(agents_dir));

    for warning in &loaded_agents_result.warnings {
        log::warn!("AGENTS.md load warning: {}", warning);
    }

    let cached_agents_chain = loaded_agents_result.merged_chain;

    let cached_available_skills = crate::protocol::skills::render_available_skills_preamble(cwd);

    (cached_agents_chain, cached_available_skills)
}

// region:    --- Tests

#[cfg(all(test, feature = "integration"))]
#[path = "../../test/conversation/runtime_build_integration/test.rs"]
mod runtime_build_integration_test;

// endregion: --- Tests
