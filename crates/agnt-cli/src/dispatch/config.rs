//! clap-based config, UI policy, theme, and compaction resolution for the CLI.

use std::time::Duration;

use agnt_core::compaction::CompactionStrategy;
use agnt_core::config::{self, CompactionConfig, Config, PluginConfig};
use agnt_core::session::StoreType;
use agnt_tty::policy::{UiPolicy, Verbosity};
use agnt_tui::rendering::theme::ThemeName;

use crate::AgentArgs;
use crate::error::{Error, Result};

// region:    --- Config

/// Resolve the runtime config and plugin config from config.toml and clap flags.
///
/// Resolution pipeline:
/// 1. Load `PluginConfig` from config.toml and resolve the active model role.
///    `--model` selects the role; otherwise `models.default` is used.
/// 2. Apply clap flag overrides (temperature, max-context/output-tokens, etc.).
/// 3. Validate and return.
pub fn resolve_config_from_clap(args: &AgentArgs) -> Result<(Config, PluginConfig)> {
    let config_path = config::toml_config::config_path()
        .map_err(|e| Error::Custom(format!("Cannot determine config path: {e}")))?;
    let (mut config, plugin_config) =
        config::load_and_resolve_config(&config_path, args.model.as_deref())
            .map_err(map_config_load_error)?;

    apply_cli_flags_from_clap(&mut config, args);

    config
        .validate()
        .map_err(|msg| Error::Custom(format!("Config validation failed: {msg}")))?;

    Ok((config, plugin_config))
}

/// Map a shared config-load error to a CLI error.
fn map_config_load_error(err: config::ConfigLoadError) -> Error {
    use config::ConfigLoadError;
    let message = match err {
        ConfigLoadError::NotFound(_) => "No configuration found. Run `agent config init` to generate a starter config.toml, or create one manually at ~/.config/nu-agent/config.toml".to_string(),
        ConfigLoadError::Toml(e) => format!("Failed to load config.toml: {e}"),
        ConfigLoadError::UnknownRole(role) => format!(
            "Unknown model role: '{role}'. Available roles: see models in config.toml"
        ),
        ConfigLoadError::NoDefaultModel => "No model configured. Edit config.toml to add a model:\n  [models.default]\n  model = \"ollama-cloud/glm-5.2\"\n\nRun `agent models sync && agent models list` to see available models.".to_string(),
        ConfigLoadError::Resolve(msg) => format!("Failed to resolve model: {msg}"),
        ConfigLoadError::Vault(e) => e.to_string(),
    };
    Error::Custom(message)
}

/// Apply clap flag overrides to a Config (highest precedence).
pub fn apply_cli_flags_from_clap(config: &mut Config, args: &AgentArgs) {
    if let Some(api_key) = &args.api_key {
        config.api_key = Some(api_key.clone());
    }
    if let Some(base_url) = &args.base_url {
        config.base_url = Some(base_url.clone());
    }
    if let Some(temperature) = args.temperature {
        config.temperature = Some(temperature);
    }
    if let Some(max_context) = args.max_context_tokens.and_then(|v| u32::try_from(v).ok()) {
        config.max_context_tokens = Some(max_context);
    }
    if let Some(max_output) = args.max_output_tokens.and_then(|v| u32::try_from(v).ok()) {
        config.max_output_tokens = Some(max_output);
    }
    if let Some(max_turns) = args.max_turns {
        config.max_tool_turns = Some(max_turns);
    }

    // --repetition-guard: enable/disable repetition guard ("on"/"off").
    if let Some(val) = &args.repetition_guard {
        config.repetition_guard = match val.as_str() {
            "on" => Some(true),
            "off" => Some(false),
            _ => None,
        };
    }
}

// endregion: --- Config

// region:    --- UI policy

/// Resolve the UI policy from clap `--quiet` / `--verbose` flags.
pub fn resolve_ui_policy_from_clap(args: &AgentArgs) -> UiPolicy {
    let quiet = args.quiet;
    let verbosity = if quiet {
        Verbosity::Quiet
    } else {
        match args.verbose {
            0 => Verbosity::Normal,
            1 => Verbosity::Verbose,
            2 => Verbosity::VeryVerbose,
            _ => Verbosity::Trace,
        }
    };
    UiPolicy { quiet, verbosity }
}

// endregion: --- UI policy

// region:    --- Theme

/// Resolve the effective TUI theme name with precedence:
/// 1. `--theme` CLI flag (validated; unknown value = error)
/// 2. `theme` in config.toml (unknown value = ignored with a log warning)
/// 3. Saved preference file (written by the picker)
/// 4. Default: CatppuccinMocha
pub fn resolve_theme_from_clap(
    theme: Option<&str>,
    plugin_config: &PluginConfig,
) -> Result<ThemeName> {
    if let Some(flag) = theme {
        return ThemeName::from_name(flag).ok_or_else(|| {
            Error::Custom(format!(
                "Unknown theme: '{flag}'. Valid values: catppuccin-mocha, catppuccin-latte, catppuccin-frappe, catppuccin-macchiato"
            ))
        });
    }

    if let Some(raw) = plugin_config.theme.as_deref() {
        if let Some(name) = ThemeName::from_name(raw) {
            return Ok(name);
        }
        log::warn!("Unknown theme in config.toml: '{raw}'; using default");
    }

    if let Ok(pref) = agnt_core::theme_pref::ThemePreference::load()
        && let Some(raw) = pref.theme.as_deref()
        && let Some(name) = ThemeName::from_name(raw)
    {
        return Ok(name);
    }

    Ok(ThemeName::default())
}

// endregion: --- Theme

// region:    --- Compaction

/// Extract compaction-related clap flags into a `CompactionConfig`.
pub fn extract_compaction_flags_from_clap(args: &AgentArgs) -> Result<CompactionConfig> {
    let strategy = match args.compaction_strategy.as_deref() {
        Some(s) => Some(
            parse_strategy_from_str(s)
                .map_err(|msg| Error::Custom(format!("Invalid --compaction-strategy: {msg}")))?,
        ),
        None => None,
    };

    Ok(CompactionConfig {
        strategy,
        proactive_threshold_pct: args.proactive_threshold_pct,
    })
}

/// Parse a compaction strategy string into a `CompactionStrategy` enum.
///
/// Uses serde deserialization so the canonical name and aliases stay in sync
/// with the enum definition.
fn parse_strategy_from_str(s: &str) -> std::result::Result<CompactionStrategy, String> {
    serde_json::from_value(serde_json::Value::String(s.to_string()))
        .map_err(|_| format!("Unknown compaction strategy '{s}'. Valid values: sliding_summary"))
}

// endregion: --- Compaction

// region:    --- Session store

/// Resolve the session store type from the clap `--store` flag.
pub fn resolve_store_type_from_clap(args: &AgentArgs) -> Result<StoreType> {
    match args.store.as_deref() {
        Some(s) => s
            .parse()
            .map_err(|e: String| Error::Custom(format!("Invalid --store value: {e}"))),
        None => Ok(StoreType::Sqlite),
    }
}

// endregion: --- Session store

// region:    --- Tool timeout

/// Extract the tool execution timeout from the clap `--tool-timeout` flag.
pub fn extract_tool_timeout_from_clap(args: &AgentArgs) -> Result<Duration> {
    match args.tool_timeout.as_deref() {
        Some(s) => parse_duration(s).map_err(Error::Custom),
        None => Ok(Duration::from_secs(30)),
    }
}

/// Parse a Nushell-style duration string (e.g. `30sec`, `500ms`, `2min`).
fn parse_duration(input: &str) -> std::result::Result<Duration, String> {
    let trimmed = input.trim();
    let split = trimmed
        .find(|c: char| c.is_alphabetic() || c == 'µ')
        .unwrap_or(trimmed.len());
    let (num_str, unit) = trimmed.split_at(split);
    let value: f64 = num_str
        .trim()
        .parse()
        .map_err(|_| format!("invalid duration '{input}'"))?;
    let unit = unit.trim().to_lowercase();
    let seconds = match unit.as_str() {
        "" | "s" | "sec" | "secs" | "second" | "seconds" => value,
        "ms" | "milli" | "millis" | "millisecond" | "milliseconds" => value / 1_000.0,
        "us" | "µs" | "micro" | "micros" | "microsecond" | "microseconds" => value / 1_000_000.0,
        "ns" | "nano" | "nanos" | "nanosecond" | "nanoseconds" => value / 1_000_000_000.0,
        "m" | "min" | "mins" | "minute" | "minutes" => value * 60.0,
        "h" | "hr" | "hrs" | "hour" | "hours" => value * 3_600.0,
        "d" | "day" | "days" => value * 86_400.0,
        "w" | "wk" | "week" | "weeks" => value * 604_800.0,
        _ => return Err(format!("unknown duration unit '{unit}' in '{input}'")),
    };
    if !seconds.is_finite() || seconds < 0.0 {
        return Err(format!("invalid duration '{input}'"));
    }
    Ok(Duration::from_secs_f64(seconds))
}

// endregion: --- Tool timeout
