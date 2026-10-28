use nu_plugin::EvaluatedCall;
use std::path::Path;

use nu_protocol::{LabeledError, Value};

use agnt_core::config::{self, Config, PluginConfig};

pub(crate) use agnt_core::conversation::runtime_build::{
    apply_persona_config, apply_persona_model, build_preamble_cache,
};

fn get_string_flag(call: &EvaluatedCall, name: &str) -> Option<String> {
    call.get_flag(name)
        .ok()
        .flatten()
        .and_then(|v: Value| v.as_str().map(|s| s.to_string()).ok())
}

/// Resolve configuration from all sources with proper precedence.
///
/// `config_path` is the config file the entry point discovered from the resolved
/// [`Paths`](agnt_core::config::Paths); this function never re-derives it.
///
/// Resolution pipeline:
/// 1. Load PluginConfig from config.toml and resolve the active model role
///    through the shared `agnt-core` loader. The `--model` flag selects the
///    role; otherwise `models.default` is used.
/// 2. Apply CLI flag overrides (temperature, max-context/output-tokens, etc.).
/// 3. Validate and return.
pub fn resolve_config(
    call: &EvaluatedCall,
    config_path: &Path,
) -> Result<(Config, PluginConfig), LabeledError> {
    let model_override = get_string_flag(call, "model");
    let (mut config, plugin_config) =
        config::load_and_resolve_config(config_path, model_override.as_deref())
            .map_err(|e| map_config_load_error(e, call))?;

    // Apply CLI flag overrides (highest precedence).
    apply_cli_flags(&mut config, call);

    // Validate final config
    config
        .validate()
        .map_err(|msg| LabeledError::new("Config validation failed").with_label(msg, call.head))?;

    Ok((config, plugin_config))
}

/// Map a shared config-load error to a labeled plugin error.
fn map_config_load_error(err: config::ConfigLoadError, call: &EvaluatedCall) -> LabeledError {
    use config::ConfigLoadError;
    match err {
        ConfigLoadError::NotFound(_) => LabeledError::new("No configuration found")
            .with_label("Run `agent config init` to generate a starter config.toml, or create one manually at ~/.config/nu-agent/config.toml", call.head),
        ConfigLoadError::Toml(e) => {
            LabeledError::new(format!("Failed to load config.toml: {e}"))
        }
        ConfigLoadError::UnknownRole(role) => {
            LabeledError::new(format!("Unknown model role: '{role}'"))
                .with_label("Available roles: see models in config.toml", call.head)
        }
        ConfigLoadError::NoDefaultModel => LabeledError::new("No model configured")
            .with_label("Edit config.toml to add a model:\n  [models.default]\n  model = \"ollama-cloud/glm-5.2\"\n\nRun `agent models sync && agent models list` to see available models.", call.head),
        ConfigLoadError::Resolve(msg) => {
            LabeledError::new("Failed to resolve model").with_label(msg, call.head)
        }
        ConfigLoadError::Vault(e) => LabeledError::new(e.to_string()),
        ConfigLoadError::Paths(e) => {
            LabeledError::new(format!("Failed to resolve application paths: {e}"))
        }
    }
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

    // --repetition-guard: enable/disable repetition guard ("on"/"off").
    if let Some(val) = get_string_flag(call, "repetition-guard") {
        config.repetition_guard = match val.as_str() {
            "on" => Some(true),
            "off" => Some(false),
            _ => None,
        };
    }
}

#[cfg(test)]
#[path = "../../../test/command/agent/runtime_build.rs"]
mod runtime_build_test;
