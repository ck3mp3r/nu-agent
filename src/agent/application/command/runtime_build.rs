use nu_plugin::EvaluatedCall;
use nu_protocol::{LabeledError, Value};

use crate::agent::protocol::preamble::{
    PreambleDefaults, UserPreambleInput, classify_model_family, resolve_preamble,
};
use crate::config::{CompactionConfig, Config, PluginConfig};
use crate::compaction::CompactionParams;

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
pub(crate) fn extract_flag_config(call: &EvaluatedCall) -> Config {
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
    }
}

/// NEW resolution flow using PluginConfig structure
pub(crate) fn resolve_with_new_config(
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
pub(crate) fn resolve_with_old_config(
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
