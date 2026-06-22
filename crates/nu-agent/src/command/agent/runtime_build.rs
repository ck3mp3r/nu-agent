use nu_plugin::{EngineInterface, EvaluatedCall};
use nu_protocol::{LabeledError, Value};

use nu_agent_core::compaction::CompactionParams;
use nu_agent_core::config::{CompactionConfig, Config, PluginConfig};
use nu_agent_core::conversation::runtime::AgentConversationRuntime;
use nu_agent_core::protocol::preamble::{
    PreambleDefaults, UserPreambleInput, classify_model_family, resolve_preamble,
};

/// Trait abstracting the engine interface functionality needed for config resolution.
///
/// This allows us to mock the EngineInterface for testing without needing
/// a real Nushell engine instance.
pub trait EngineConfigInterface {
    fn get_plugin_config(&self) -> Result<Option<Value>, LabeledError>;
}

impl EngineConfigInterface for EngineInterface {
    fn get_plugin_config(&self) -> Result<Option<Value>, LabeledError> {
        // Convert ShellError to LabeledError
        self.get_plugin_config()
            .map_err(|e| LabeledError::new(format!("Failed to get plugin config: {}", e)))
    }
}

/// Resolve configuration from all sources with proper precedence.
///
/// NEW Resolution pipeline:
/// 1. Parse PluginConfig from $env.config.plugins.agent (if present)
/// 2. Determine active model:
///    - If --model flag provided: use it (provider/model format)
///    - Else if --small flag provided: use small_model from PluginConfig
///    - Else use config.model (default)
/// 3. Call PluginConfig::resolve_model() to get base Config
/// 4. Merge with flag overrides (temperature, max-context/output-tokens, etc.)
/// 5. Validate and return
///
/// FALLBACK for backward compatibility:
/// - If plugin config doesn't have new structure (no "providers" field)
/// - Fall back to OLD Config::from_plugin_config() behavior
/// - Model override remains authoritative via --model (provider/model format)
///
/// # Arguments
/// * `engine` - Engine interface for accessing plugin config
/// * `call` - The EvaluatedCall containing command flags
///
/// # Returns
/// Fully resolved and validated Config, or error if validation fails
pub fn resolve_config<E: EngineConfigInterface>(
    engine: &E,
    call: &EvaluatedCall,
) -> Result<Config, LabeledError> {
    // Step 1: Get plugin config value (if present)
    let plugin_config_opt = engine.get_plugin_config()?;

    // Step 2: Try NEW plugin config structure first
    if let Some(ref plugin_value) = plugin_config_opt {
        // Try to parse as NEW PluginConfig structure
        if let Ok(plugin_config) = PluginConfig::from_plugin_config(plugin_value) {
            // NEW FLOW: Use PluginConfig
            return resolve_with_new_config(plugin_config, call);
        }
        // If parsing failed, fall through to OLD flow
    }

    // Step 3: FALLBACK to OLD flow for backward compatibility
    resolve_with_old_config(plugin_config_opt, call)
}

/// Extract configuration from command-line flags.
///
/// Reads flags from the EvaluatedCall and returns a Config with values for
/// provided flags and None for unprovided flags.
///
/// # Arguments
/// * `call` - The EvaluatedCall containing command flags
///
/// # Returns
/// Config with values from flags or Config::default() fields for unprovided flags
pub fn extract_flag_config(call: &EvaluatedCall) -> Config {
    // Helper to safely extract string flag
    fn get_string_flag(call: &EvaluatedCall, name: &str) -> Option<String> {
        call.get_flag(name)
            .ok()
            .flatten()
            .and_then(|v: Value| v.as_str().map(|s| s.to_string()).ok())
    }

    // Helper to safely extract float flag
    fn get_float_flag(call: &EvaluatedCall, name: &str) -> Option<f64> {
        call.get_flag(name)
            .ok()
            .flatten()
            .and_then(|v: Value| v.as_float().ok())
    }

    // Helper to safely extract u32 flag (from i64, rejecting negatives)
    fn get_u32_flag(call: &EvaluatedCall, name: &str) -> Option<u32> {
        call.get_flag(name)
            .ok()
            .flatten()
            .and_then(|v: Value| v.as_int().ok())
            .and_then(|i| if i >= 0 { Some(i as u32) } else { None })
    }

    // Extract all flags
    let model = get_string_flag(call, "model").unwrap_or_default();
    let api_key = get_string_flag(call, "api-key");
    let base_url = get_string_flag(call, "base-url");
    let temperature = get_float_flag(call, "temperature");
    let max_context_tokens = get_u32_flag(call, "max-context-tokens");
    let max_output_tokens = get_u32_flag(call, "max-output-tokens");
    let max_tool_turns = get_u32_flag(call, "max-turns");

    Config {
        provider: String::new(),
        provider_impl: None,
        model,
        api_key,
        base_url,
        temperature,
        max_tokens: None,
        max_context_tokens,
        max_output_tokens,
        max_tool_turns,
        preamble: None,
        read_timeout_secs: None,
    }
}

/// NEW resolution flow using PluginConfig structure
pub fn resolve_with_new_config(
    plugin_config: PluginConfig,
    call: &EvaluatedCall,
) -> Result<Config, LabeledError> {
    // Helper to get string flag
    fn get_string_flag(call: &EvaluatedCall, name: &str) -> Option<String> {
        call.get_flag(name)
            .ok()
            .flatten()
            .and_then(|v: Value| v.as_str().map(|s| s.to_string()).ok())
    }

    // Helper to get bool flag (switch)
    fn get_bool_flag(call: &EvaluatedCall, name: &str) -> bool {
        call.get_flag(name).ok().flatten().unwrap_or(false)
    }

    // Determine which model to use (priority: --model > --small > config.model)
    let model_ref = if let Some(model_flag) = get_string_flag(call, "model") {
        // --model flag takes highest priority
        model_flag
    } else if get_bool_flag(call, "small") {
        // --small flag uses small_model from config
        plugin_config.small_model.clone().ok_or_else(|| {
            LabeledError::new("No small model configured").with_label(
                "Set 'small_model' in plugin config to use --small flag",
                call.head,
            )
        })?
    } else {
        // Use default model from config
        plugin_config.model.clone()
    };

    // Resolve model to Config using PluginConfig
    let mut config = plugin_config
        .resolve_model(&model_ref)
        .map_err(|msg| LabeledError::new("Failed to resolve model").with_label(msg, call.head))?;

    // Resolve preamble via canonical resolver.
    if let Some((provider_name, model_name)) = model_ref.split_once('/')
        && let Some(provider_cfg) = plugin_config.providers.get(provider_name)
    {
        let model_cfg = provider_cfg.models.get(model_name);
        let defaults = PreambleDefaults::builtin();
        config.preamble = resolve_preamble(
            UserPreambleInput {
                provider: provider_name.to_string(),
                model_family: Some(classify_model_family(provider_name, model_name)),
                user_provider_preamble: provider_cfg.preamble.clone(),
                user_provider_family_preamble: model_cfg.and_then(|cfg| cfg.preamble.clone()),
            },
            &defaults,
        );
    }

    // Step 3: Apply flag overrides for optional fields
    // These override any values from PluginConfig
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

    // Step 4: Validate final config
    config
        .validate()
        .map_err(|msg| LabeledError::new("Config validation failed").with_label(msg, call.head))?;

    Ok(config)
}

/// OLD resolution flow for backward compatibility
pub fn resolve_with_old_config(
    plugin_config_opt: Option<Value>,
    call: &EvaluatedCall,
) -> Result<Config, LabeledError> {
    // Step 1: Extract flag config first
    let flag_config = extract_flag_config(call);
    let model_override = if flag_config.model.is_empty() {
        None
    } else {
        let (provider, model) = flag_config.model.split_once('/').ok_or_else(|| {
            LabeledError::new("Invalid --model format")
                .with_label("Expected provider/model (e.g. openai/gpt-4)", call.head)
        })?;

        if provider.is_empty() || model.is_empty() {
            return Err(LabeledError::new("Invalid --model format")
                .with_label("Provider and model must both be non-empty", call.head));
        }

        Some((provider.to_string(), model.to_string()))
    };

    // Step 2: Determine provider/model for env lookup
    // Use plugin config if available, then flags, then default
    let (provider_hint, model_hint) = if let Some(ref plugin_value) = plugin_config_opt {
        // Try to extract provider/model from plugin config for env lookup
        let plugin_parsed = Config::from_plugin_config(plugin_value)?;
        (plugin_parsed.provider.clone(), plugin_parsed.model.clone())
    } else if let Some((provider, model)) = model_override.as_ref() {
        (provider.clone(), model.clone())
    } else {
        ("openai".to_string(), "gpt-4".to_string())
    };

    // Step 3: Start with defaults and merge environment config
    let env_config = Config::from_env(&provider_hint, &model_hint);
    let mut config = Config::default().merge(env_config);

    // Step 4: Merge plugin config if present
    if let Some(plugin_value) = plugin_config_opt {
        let plugin_config = Config::from_plugin_config(&plugin_value)?;
        config = config.merge(plugin_config);
    }

    // Step 5: Merge flag config (highest precedence) - only if values are non-empty
    // For required fields, only override if non-empty
    if let Some((provider, model)) = model_override {
        config.provider = provider;
        config.model = model;
    }
    // For optional fields, use standard merge
    config.api_key = flag_config.api_key.or(config.api_key);
    config.base_url = flag_config.base_url.or(config.base_url);
    config.temperature = flag_config.temperature.or(config.temperature);
    config.max_tokens = flag_config.max_tokens.or(config.max_tokens);
    config.max_context_tokens = flag_config.max_context_tokens.or(config.max_context_tokens);
    config.max_output_tokens = flag_config.max_output_tokens.or(config.max_output_tokens);
    config.max_tool_turns = flag_config.max_tool_turns.or(config.max_tool_turns);

    // Step 6: Validate final config
    config
        .validate()
        .map_err(|msg| LabeledError::new("Config validation failed").with_label(msg, call.head))?;

    Ok(config)
}

/// Apply persona model override if CLI --model was not provided.
/// Returns true if persona model was applied.
pub(crate) fn apply_persona_model(
    config: &mut Config,
    persona_model: Option<&str>,
    cli_model_provided: bool,
) -> bool {
    if cli_model_provided {
        return false;
    }
    let Some(m) = persona_model else {
        return false;
    };
    let Some((provider, model)) = m.split_once('/') else {
        return false;
    };
    config.provider = provider.to_string();
    config.model = model.to_string();
    config.provider_impl = None;
    log::debug!("apply_persona_model: overriding to provider={provider}, model={model}");
    true
}

/// Merge two `CompactionConfig`s with `override_cfg` taking precedence.
///
/// For each field, if `override_cfg` has `Some`, use it; otherwise keep `base`.
/// Both inputs are `Option<&CompactionConfig>` — `None` means "no config from this source".
pub(crate) fn merge_compaction_configs(
    base: Option<&CompactionConfig>,
    override_cfg: &CompactionConfig,
) -> CompactionConfig {
    let base = base.cloned().unwrap_or_default();
    CompactionConfig {
        strategy: override_cfg.strategy.or(base.strategy),
        keep_recent: override_cfg.keep_recent.or(base.keep_recent),
        token_budget: override_cfg.token_budget.or(base.token_budget),
        proactive_threshold_pct: override_cfg
            .proactive_threshold_pct
            .or(base.proactive_threshold_pct),
    }
}

/// Build a `CompactionParams` from a merged `CompactionConfig`.
///
/// Applies `CompactionConfig` field overrides on top of `CompactionParams::default()`.
/// Fields that are `None` in the config use the `CompactionParams` defaults.
pub(crate) fn build_compaction_params(merged: &CompactionConfig) -> CompactionParams {
    let mut config = CompactionParams::default();

    if let Some(strategy) = merged.strategy {
        config.compaction_strategy = strategy;
    }
    if let Some(keep_recent) = merged.keep_recent {
        config.keep_recent = keep_recent;
    }
    if let Some(token_budget) = merged.token_budget {
        config.token_budget = Some(token_budget);
    }

    config
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
    pub(crate) runtime: tokio::runtime::Runtime,
    pub(crate) config: Config,
    pub(crate) plugin_config_value: Option<nu_protocol::Value>,
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
    pub(crate) store: nu_agent_core::session::SessionStore,
    pub(crate) final_session_id: Option<String>,
    pub(crate) context_window_max_tokens: u64,
    pub(crate) compaction_threshold_pct: f64,
    pub(crate) compaction_count: usize,
    pub(crate) compaction_strategy: nu_agent_core::compaction::CompactionStrategy,
    pub(crate) effective_permissions: nu_agent_core::tools::authz::PermissionsConfig,
    pub(crate) permissions_startup_summary: String,
    pub(crate) persona_body: Option<String>,
    pub(crate) agent_identity: Option<String>,
    pub(crate) agent_description: Option<String>,
    pub(crate) cached_agents_chain: Option<String>,
    pub(crate) cached_available_skills: Option<String>,
    pub(crate) cached_sub_agent_instruction: Option<String>,
    pub(crate) mailbox_rx:
        Option<std::sync::mpsc::Receiver<nu_agent_core::mailbox::IncomingMessage>>,
    pub(crate) available_agents: Vec<nu_agent_core::protocol::persona::PersonaSummary>,
    pub(crate) agents_config: nu_agent_core::config::AgentsConfig,
}

pub(crate) fn build_runtime(params: RuntimeBuildParams) -> AgentConversationRuntime {
    use nu_agent_core::config::PluginConfig;
    use nu_agent_core::conversation::{
        compaction::state::CompactionState, state::mcp::McpState, state::memory::MemoryState,
        state::multi_agent::MultiAgentState, state::permission::PermissionState,
        state::persona::PersonaState, state::provider::ProviderState, state::tool::ToolState,
    };

    // CRITICAL: extract cache_dir BEFORE moving params.store into the struct literal
    // because params.store.cache_dir() and params.store cannot both be used after move
    let cache_dir = params.store.cache_dir().to_path_buf();

    AgentConversationRuntime {
        runtime: params.runtime,
        tool_server_handle: params.tool_server_handle,
        provider_state: ProviderState::new(
            params.config,
            params
                .plugin_config_value
                .as_ref()
                .and_then(|value| PluginConfig::from_plugin_config(value).ok()),
        ),
        tool_state: ToolState::new(
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
        ),
        engine: params.engine,
        store: params.store,
        final_session_id: params.final_session_id,
        compaction_state: CompactionState::new(
            params.context_window_max_tokens,
            params.compaction_threshold_pct,
            params.compaction_count,
            params.compaction_strategy,
        ),
        permission_state: PermissionState::new(
            params.effective_permissions,
            nu_agent_core::tools::authz::SessionGrantCache::default(),
            params.permissions_startup_summary,
        ),
        memory_state: MemoryState::new(cache_dir),
        persona_state: PersonaState::new(
            params.persona_body,
            params.agent_identity,
            params.agent_description,
            params.cached_agents_chain,
            params.cached_available_skills,
            params.cached_sub_agent_instruction,
        ),
        multi_agent_state: MultiAgentState::new(
            params.mailbox_rx,
            params.available_agents,
            params.agents_config,
        ),
        interactive_pending: None,
    }
}
