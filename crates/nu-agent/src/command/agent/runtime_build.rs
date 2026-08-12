use std::sync::{Arc, Mutex};

use nu_plugin::EvaluatedCall;
use nu_protocol::{LabeledError, Value};

use nu_agent_core::config::{
    self, Config, ModelRoleConfig, PluginConfig, defaults,
    models_cache::{ModelsCache, ModelsCacheError},
    secrets::{SecretStore, SecretStoreError},
};
use nu_agent_core::conversation::runtime::AgentConversationRuntime;
use nu_agent_core::protocol::preamble::{
    PreambleDefaults, UserPreambleInput, classify_model_family, resolve_preamble,
};
use nu_agent_core::session::SessionStoreImpl;
use nu_agent_core::tools::mcp::circuit_breaker::McpCircuitBreaker;

fn get_string_flag(call: &EvaluatedCall, name: &str) -> Option<String> {
    call.get_flag(name)
        .ok()
        .flatten()
        .and_then(|v: Value| v.as_str().map(|s| s.to_string()).ok())
}

/// Resolve configuration from all sources with proper precedence.
///
/// Resolution pipeline:
/// 1. Parse PluginConfig from config.toml (if present)
/// 2. Determine active model:
///    - If --model flag provided: use it (provider/model format)
///    - Else use models.default from PluginConfig
/// 3. Call PluginConfig::resolve_model() to get base Config
/// 4. Merge with flag overrides (temperature, max-context/output-tokens, etc.)
/// 5. Validate and return
///
/// When no plugin config is present, --model is required. Config is built
/// directly from env vars and CLI flags without going through PluginConfig.
pub fn resolve_config(call: &EvaluatedCall) -> Result<(Config, PluginConfig), LabeledError> {
    let config_path = config::toml_config::config_path()
        .map_err(|e| LabeledError::new(format!("Cannot determine config path: {e}")))?;
    if !config_path.exists() {
        return Err(LabeledError::new("No configuration found")
            .with_label("Run `agent config init` to generate a starter config.toml, or create one manually at ~/.config/nu-agent/config.toml", call.head));
    }
    let mut plugin_config = config::toml_config::load()
        .map_err(|e| LabeledError::new(format!("Failed to load config.toml: {e}")))?;
    match SecretStore::load() {
        Ok(store) => plugin_config.secret_store = Some(store),
        Err(SecretStoreError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => log::warn!("Failed to load secret store: {e}"),
    }
    match ModelsCache::load() {
        Ok(cache) => plugin_config.models_cache = Some(cache),
        Err(ModelsCacheError::NotFound(_)) => {}
        Err(e) => log::warn!("Failed to load models cache: {e}"),
    }
    let config = resolve_with_new_config(plugin_config.clone(), call)?;
    Ok((config, plugin_config))
}
/// Resolve preamble for a specific provider/model pair.
///
/// Looks up the provider config and model config from `plugin_config` and
/// resolves the preamble using the canonical `resolve_preamble` function.
/// Returns `None` if the provider is not found in the plugin config.
pub(crate) fn resolve_preamble_for_model(
    plugin_config: &PluginConfig,
    provider_name: &str,
    model_name: &str,
) -> Option<String> {
    let provider_cfg = plugin_config.providers.get(provider_name)?;
    let model_cfg = provider_cfg.models.get(model_name);
    let defaults = PreambleDefaults::builtin();
    resolve_preamble(
        UserPreambleInput {
            provider: provider_name.to_string(),
            model_family: Some(classify_model_family(provider_name, model_name)),
            user_provider_preamble: provider_cfg.preamble.clone(),
            user_provider_family_preamble: model_cfg.and_then(|cfg| cfg.preamble.clone()),
        },
        &defaults,
    )
}

/// NEW resolution flow using PluginConfig structure
pub fn resolve_with_new_config(
    plugin_config: PluginConfig,
    call: &EvaluatedCall,
) -> Result<Config, LabeledError> {
    // Determine which model role to use (priority: --model flag > models.default)
    let role_config = if let Some(model_flag) = get_string_flag(call, "model") {
        if model_flag.contains('/') {
            // Literal provider/model string — use as-is with default overrides
            ModelRoleConfig {
                model: model_flag,
                ..Default::default()
            }
        } else {
            // Role label — look up from plugin_config.models
            plugin_config
                .models
                .get(&model_flag)
                .ok_or_else(|| {
                    LabeledError::new(format!("Unknown model role: '{model_flag}'"))
                        .with_label("Available roles: see models in config.toml", call.head)
                })?
                .clone()
        }
    } else {
        // Use default model from config
        plugin_config
            .models
            .get("default")
            .ok_or_else(|| {
                LabeledError::new("No model configured")
                    .with_label("Edit config.toml to add a model:\n  [models.default]\n  model = \"ollama-cloud/glm-5.2\"\n\nRun `agent models sync && agent models list` to see available models.", call.head)
            })?
            .clone()
    };

    // Resolve model to Config using PluginConfig
    let mut config = plugin_config
        .resolve_model(&role_config)
        .map_err(|msg| LabeledError::new("Failed to resolve model").with_label(msg, call.head))?;

    // Resolve preamble via canonical resolver.
    if let Some((provider_name, model_name)) = role_config.model.split_once('/') {
        config.preamble = resolve_preamble_for_model(&plugin_config, provider_name, model_name);
    }

    // Apply CLI flag overrides (highest precedence).
    apply_cli_flags(&mut config, call);

    // Validate final config
    config
        .validate()
        .map_err(|msg| LabeledError::new("Config validation failed").with_label(msg, call.head))?;

    Ok(config)
}

/// Apply CLI flag overrides to a Config.
///
/// These are the highest-priority overrides, applied last in the resolution chain.
/// Fields: --api-key, --base-url, --temperature, --max-context-tokens,
/// --max-output-tokens, --max-turns
pub(crate) fn apply_cli_flags(config: &mut Config, call: &EvaluatedCall) {
    if let Some(api_key) = get_string_flag(call, "api-key") {
        config.api_key = Some(api_key);
    }
    if let Some(base_url) = get_string_flag(call, "base-url") {
        config.base_url = Some(base_url);
    }
    if let Some(temperature) = call
        .get_flag::<Value>("temperature")
        .ok()
        .flatten()
        .and_then(|v| v.as_float().ok())
    {
        config.temperature = Some(temperature);
    }
    if let Some(max_context) = call
        .get_flag::<Value>("max-context-tokens")
        .ok()
        .flatten()
        .and_then(|v| v.as_int().ok())
        .and_then(|i| if i >= 0 { Some(i as u32) } else { None })
    {
        config.max_context_tokens = Some(max_context);
    }
    if let Some(max_output) = call
        .get_flag::<Value>("max-output-tokens")
        .ok()
        .flatten()
        .and_then(|v| v.as_int().ok())
        .and_then(|i| if i >= 0 { Some(i as u32) } else { None })
    {
        config.max_output_tokens = Some(max_output);
    }
    if let Some(max_turns) = call
        .get_flag::<Value>("max-turns")
        .ok()
        .flatten()
        .and_then(|v| v.as_int().ok())
        .and_then(|i| if i >= 0 { Some(i as u32) } else { None })
    {
        config.max_tool_turns = Some(max_turns);
    }

    // --a2a-port: enables A2A and optionally sets the port.
    // Applied here (after apply_persona_model) so CLI values survive
    // config replacement by persona model resolution.
    if let Some(port_val) = call.get_flag::<i64>("a2a-port").ok().flatten()
        && (0..=65535).contains(&port_val)
    {
        config.a2a_enabled = Some(true);
        config.a2a_port = Some(port_val as u16);
    }
}

/// Apply persona model override if CLI --model was not provided.
///
/// `persona_model` can be:
/// - A literal `provider/model` string (contains `/`) → used as-is
/// - A role label (no `/`) → resolved through `plugin_config.models`
///
/// Returns `Ok(true)` if persona model was applied, `Ok(false)` if skipped.
/// Returns `Err` if a role label is unknown or not configured.
pub(crate) fn apply_persona_model(
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
        config.preamble = resolve_preamble_for_model(pc, provider_name, model_name);
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
pub(crate) fn apply_persona_config(
    config: &mut Config,
    persona: &nu_agent_core::protocol::persona::ParsedPersona,
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
/// Returns (cached_agents_chain, cached_available_skills, cached_sub_agent_instruction).
pub(crate) fn build_preamble_cache(
    cwd: &std::path::Path,
    parent_name: Option<&str>,
) -> (Option<String>, Option<String>, Option<String>) {
    let loaded_agents_result = nu_agent_core::protocol::agents::load_agents_chain_for_cwd(cwd);

    for warning in &loaded_agents_result.warnings {
        log::warn!("AGENTS.md load warning: {}", warning);
    }

    let cached_agents_chain = loaded_agents_result.merged_chain;

    let cached_available_skills =
        nu_agent_core::protocol::skills::render_available_skills_preamble(cwd);

    let cached_sub_agent_instruction = parent_name.map(|parent| {
        format!(
            "You are a sub-agent. When you have completed your task, report your results back \
             to your parent agent using the send_message tool with kind 'completion': \
             send_message(to: \"{parent}\", message: \"<your results>\", kind: \"completion\"). \
             If you are blocked and need a decision from your parent, use kind 'question': \
             send_message(to: \"{parent}\", message: \"<your question>\", kind: \"question\"). \
             Work autonomously — only use 'question' when truly blocked."
        )
    });

    (
        cached_agents_chain,
        cached_available_skills,
        cached_sub_agent_instruction,
    )
}

pub(crate) struct RuntimeBuildParams {
    pub(crate) runtime: tokio::runtime::Handle,
    pub(crate) config: Config,
    pub(crate) plugin_config: Option<PluginConfig>,
    pub(crate) tool_definitions: Vec<nu_agent_core::types::ToolDefinition>,
    pub(crate) baseline_tool_definitions: Vec<nu_agent_core::types::ToolDefinition>,
    pub(crate) closure_registry: nu_agent_core::tools::closure::ClosureRegistry,
    pub(crate) mcp_runtime: Option<nu_agent_core::tools::mcp::runtime::McpRuntime>,
    pub(crate) tool_server_handle: rig::tool::server::ToolServerHandle,
    pub(crate) mcp_lifecycle_projection:
        Vec<nu_agent_core::tools::mcp::runtime::McpServerLifecycle>,
    pub(crate) mcp_server_configs: Vec<nu_agent_core::tools::mcp::config::McpServerConfig>,
    pub(crate) mcp_caller_cwd: Option<std::path::PathBuf>,
    pub(crate) mcp_registry: nu_agent_core::tools::handler::McpToolRegistry,
    pub(crate) engine: nu_plugin::EngineInterface,
    pub(crate) store: Arc<SessionStoreImpl>,
    pub(crate) final_session_id: Option<String>,
    pub(crate) context_window_max_tokens: u64,
    pub(crate) compaction_threshold_pct: f64,
    pub(crate) compaction_strategy: nu_agent_core::compaction::CompactionStrategy,
    pub(crate) compaction_params: nu_agent_core::compaction::CompactionParams,
    pub(crate) base_permissions: nu_agent_core::tools::authz::PermissionsConfig,
    pub(crate) effective_permissions: nu_agent_core::tools::authz::PermissionsConfig,
    pub(crate) cli_permissions_overlay: Option<nu_agent_core::tools::authz::PermissionsOverlay>,
    pub(crate) permissions_startup_summary: String,
    pub(crate) persona_body: Option<String>,
    pub(crate) agent_identity: Option<String>,
    pub(crate) agent_description: Option<String>,
    pub(crate) agent_icon: Option<String>,
    pub(crate) cached_agents_chain: Option<String>,
    pub(crate) cached_available_skills: Option<String>,
    pub(crate) cached_sub_agent_instruction: Option<String>,
    pub(crate) available_agents: Vec<nu_agent_core::protocol::persona::PersonaSummary>,
    pub(crate) agents_config: nu_agent_core::config::AgentsConfig,
    pub(crate) cwd: std::path::PathBuf,
    pub(crate) bus: nu_agent_core::bus::Bus,
}

pub(crate) fn build_runtime(params: RuntimeBuildParams) -> AgentConversationRuntime {
    use nu_agent_core::conversation::{
        compaction::state::CompactionState, state::mcp::McpState, state::memory::MemoryState,
        state::multi_agent::MultiAgentState, state::permission::PermissionState,
        state::persona::PersonaState, state::provider::ProviderState, state::tool::ToolState,
    };

    // CRITICAL: clone store BEFORE moving params.store into the struct literal
    // so both AgentRuntime.store and MemoryState share the same backing store.
    let store_for_session = Arc::clone(&params.store);
    // Extract max_tool_result_bytes before params.config is moved.
    let max_tool_result_bytes = params
        .config
        .max_tool_result_bytes
        .unwrap_or(defaults::MAX_TOOL_RESULT_BYTES);

    AgentConversationRuntime {
        runtime: params.runtime,
        tool_server_handle: params.tool_server_handle,
        provider: ProviderState::new(params.config, params.plugin_config),
        tools: ToolState::new(
            params.tool_definitions,
            params.baseline_tool_definitions,
            params.closure_registry,
        ),
        mcp_state: McpState::new(
            params.mcp_runtime,
            params.mcp_lifecycle_projection,
            params.mcp_server_configs,
            params.mcp_caller_cwd,
            params.mcp_registry,
            max_tool_result_bytes,
        ),
        engine: params.engine,
        store: params.store,
        final_session_id: params.final_session_id,
        compaction: CompactionState::new(
            params.context_window_max_tokens,
            params.compaction_threshold_pct,
            params.compaction_strategy,
        ),
        permission_state: PermissionState::new(
            params.base_permissions,
            params.effective_permissions,
            params.cli_permissions_overlay,
            nu_agent_core::tools::authz::SessionGrantCache::default(),
            params.permissions_startup_summary,
        ),
        session: MemoryState::new(store_for_session),
        persona: PersonaState::new(
            params.persona_body,
            params.agent_identity,
            params.agent_description,
            params.agent_icon,
            params.cached_agents_chain,
            params.cached_available_skills,
            params.cached_sub_agent_instruction,
        ),
        multi_agent: MultiAgentState::new(params.available_agents, params.agents_config),
        compaction_params: params.compaction_params,
        cwd: params.cwd,
        interactive_pending: None,
        circuit_breaker: Arc::new(Mutex::new(McpCircuitBreaker::default())),
        doom_state: Arc::new(Mutex::new(nu_agent_core::hook::DoomLoopState::default())),
        bus: params.bus,
    }
}

#[cfg(test)]
#[path = "runtime_build_test.rs"]
mod runtime_build_test;
