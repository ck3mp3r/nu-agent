use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::utils::env_map::{EnvMap, process_env};
use crate::utils::xdg;
use crate::utils::xdg::XdgError;

use super::PluginConfig;
use super::cmd_refs::{CMD_PREFIX, CmdRunner, DEFAULT_TIMEOUT, ProcessCmdRunner, resolve_cmd_refs};
use super::defaults;
use super::types::{HookEvent, ResolvedHook};

#[derive(Debug)]
pub enum TomlConfigError {
    InvalidApiKey(String),
    CmdHelper(String),
    Hook(String),
    NotFound(PathBuf),
    Parse(toml::de::Error),
    Io(std::io::Error),
    Xdg(XdgError),
}

impl std::fmt::Display for TomlConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidApiKey(message) => write!(f, "{message}"),
            Self::CmdHelper(message) => write!(f, "{message}"),
            Self::Hook(message) => write!(f, "{message}"),
            Self::NotFound(p) => write!(f, "config not found at {}", p.display()),
            Self::Parse(e) => write!(f, "failed to parse config.toml: {e}"),
            Self::Io(e) => write!(f, "IO error reading config.toml: {e}"),
            Self::Xdg(e) => write!(f, "XDG error: {e}"),
        }
    }
}

impl std::error::Error for TomlConfigError {}

/// Returns the path to config.toml: `$XDG_CONFIG_HOME/nu-agent/config.toml`
pub fn config_path() -> Result<PathBuf, TomlConfigError> {
    config_path_with(&process_env())
}

/// Returns the path to config.toml using an explicit environment map.
pub fn config_path_with(env: &EnvMap) -> Result<PathBuf, TomlConfigError> {
    let dir = xdg::config_dir_with(env).map_err(TomlConfigError::Xdg)?;
    Ok(dir.join("nu-agent").join("config.toml"))
}

/// Load PluginConfig from config.toml.
/// Returns `PluginConfig::default()` if the file doesn't exist (not an error).
/// Returns an error only if the file exists but can't be read or parsed.
pub fn load() -> Result<PluginConfig, TomlConfigError> {
    load_from(&config_path()?)
}

/// Load PluginConfig from an explicit path.
///
/// Resolves the `cmd:` helper timeout with `AGENT_CMD_TIMEOUT_SECS` >
/// `cmd_timeout_secs` > [`DEFAULT_TIMEOUT`] precedence.
///
/// Returns `PluginConfig::default()` if the file doesn't exist (not an error).
/// Returns an error only if the file exists but can't be read or parsed.
pub fn load_from(path: &Path) -> Result<PluginConfig, TomlConfigError> {
    let timeout = resolve_cmd_timeout(path)?;
    load_from_with(path, &ProcessCmdRunner::new(timeout))
}

/// Load PluginConfig from an explicit path, resolving `cmd:` references with `runner`.
///
/// Returns `PluginConfig::default()` if the file doesn't exist (not an error).
/// Returns an error only if the file exists but can't be read, parsed, or
/// resolved.
pub fn load_from_with(
    path: &Path,
    runner: &dyn CmdRunner,
) -> Result<PluginConfig, TomlConfigError> {
    if !path.exists() {
        return Ok(PluginConfig::default());
    }
    let contents = std::fs::read_to_string(path).map_err(TomlConfigError::Io)?;
    let mut value: toml::Value = toml::from_str(&contents).map_err(TomlConfigError::Parse)?;
    validate_hooks(&value)?;
    let config: PluginConfig = toml::from_str(&contents).map_err(TomlConfigError::Parse)?;
    validate_api_keys(&config)?;
    resolve_cmd_refs(&mut value, runner).map_err(|e| TomlConfigError::CmdHelper(e.to_string()))?;
    value.try_into().map_err(TomlConfigError::Parse)
}

/// Resolve the `cmd:` helper timeout: env > config.toml > default.
fn resolve_cmd_timeout(path: &Path) -> Result<Duration, TomlConfigError> {
    resolve_cmd_timeout_with(path, &process_env())
}

/// Resolve the `cmd:` helper timeout from an explicit environment map.
///
/// The map-taking variant keeps the precedence testable without mutating the
/// process environment.
fn resolve_cmd_timeout_with(path: &Path, env: &EnvMap) -> Result<Duration, TomlConfigError> {
    if let Some(secs) = env
        .get("AGENT_CMD_TIMEOUT_SECS")
        .and_then(|raw| raw.parse::<u64>().ok())
    {
        return Ok(Duration::from_secs(secs));
    }
    if !path.exists() {
        return Ok(DEFAULT_TIMEOUT);
    }
    let contents = std::fs::read_to_string(path).map_err(TomlConfigError::Io)?;
    let value: toml::Value = toml::from_str(&contents).map_err(TomlConfigError::Parse)?;
    let secs = value
        .get("cmd_timeout_secs")
        .and_then(toml::Value::as_integer)
        .and_then(|secs| u64::try_from(secs).ok());
    Ok(secs.map_or(DEFAULT_TIMEOUT, Duration::from_secs))
}

/// Reject raw API key literals in config.toml.
///
/// Every `providers.<name>.api_key` value must be a `store:`, `env:`, or
/// `cmd:` reference. A raw literal would put a plaintext secret on disk, so it
/// is a hard error.
fn validate_api_keys(config: &PluginConfig) -> Result<(), TomlConfigError> {
    for (name, provider) in &config.providers {
        let Some(api_key) = &provider.api_key else {
            continue;
        };
        if api_key.starts_with("store:")
            || api_key.starts_with("env:")
            || api_key.starts_with(CMD_PREFIX)
        {
            continue;
        }
        return Err(TomlConfigError::InvalidApiKey(format!(
            "api_key in [providers.{name}] must be a store:, env:, or cmd: reference, not a raw value. Use: agent provider auth login {name}"
        )));
    }
    Ok(())
}

/// Validate the raw `[[hooks]]` array before the typed parse.
///
/// Runs on the raw TOML so the error can name the hook and the offending event;
/// the typed parse alone would report only the unknown variant.
fn validate_hooks(value: &toml::Value) -> Result<(), TomlConfigError> {
    let Some(hooks) = value.get("hooks") else {
        return Ok(());
    };
    let Some(entries) = hooks.as_array() else {
        return Err(TomlConfigError::Hook(
            "hooks must be an array of tables ([[hooks]])".to_string(),
        ));
    };
    for (index, entry) in entries.iter().enumerate() {
        let name = entry
            .get("name")
            .and_then(toml::Value::as_str)
            .unwrap_or_default();
        if name.is_empty() {
            return Err(TomlConfigError::Hook(format!(
                "hooks[{index}] must have a non-empty name"
            )));
        }
        let script = entry
            .get("script")
            .and_then(toml::Value::as_str)
            .unwrap_or_default();
        if script.is_empty() {
            return Err(TomlConfigError::Hook(format!(
                "hook '{name}' must have a non-empty script"
            )));
        }
        let Some(events) = entry.get("events").and_then(toml::Value::as_array) else {
            return Err(TomlConfigError::Hook(format!(
                "hook '{name}' must declare an events array"
            )));
        };
        for event in events {
            let Some(event_name) = event.as_str() else {
                return Err(TomlConfigError::Hook(format!(
                    "hook '{name}' has a non-string event name"
                )));
            };
            if HookEvent::from_name(event_name).is_none() {
                return Err(TomlConfigError::Hook(format!(
                    "hook '{name}' declares an unknown event '{event_name}'; expected one of: {}",
                    HookEvent::NAMES.join(", ")
                )));
            }
        }
    }
    Ok(())
}

/// Resolve every `[[hooks]]` entry against `config_dir`.
///
/// A relative `script` joins `config_dir`; an absolute one stays as written. An
/// absent `timeout_ms` falls back to [`defaults::HOOK_TIMEOUT_MS`].
pub fn resolve_hooks(
    config: &PluginConfig,
    config_dir: &Path,
) -> Result<Vec<ResolvedHook>, TomlConfigError> {
    config
        .hooks
        .iter()
        .map(|hook| {
            if hook.name.is_empty() {
                return Err(TomlConfigError::Hook(
                    "a [[hooks]] entry must have a non-empty name".to_string(),
                ));
            }
            if hook.script.is_empty() {
                return Err(TomlConfigError::Hook(format!(
                    "hook '{}' must have a non-empty script",
                    hook.name
                )));
            }
            let script = Path::new(&hook.script);
            let script = if script.is_absolute() {
                script.to_path_buf()
            } else {
                config_dir.join(script)
            };
            Ok(ResolvedHook {
                name: hook.name.clone(),
                events: hook.events.clone(),
                script,
                timeout: Duration::from_millis(
                    hook.timeout_ms.unwrap_or(defaults::HOOK_TIMEOUT_MS),
                ),
            })
        })
        .collect()
}

/// Resolve the configured hooks against the default config directory.
///
/// The mode setups call this: they hold a [`PluginConfig`] but not the path it
/// was loaded from, and a relative hook script resolves against the directory
/// holding config.toml.
pub fn resolve_configured_hooks(
    config: &PluginConfig,
) -> Result<Vec<ResolvedHook>, TomlConfigError> {
    let path = config_path()?;
    let config_dir = path.parent().unwrap_or_else(|| Path::new("."));
    resolve_hooks(config, config_dir)
}

#[cfg(test)]
#[path = "../../test/config/toml_config.rs"]
mod toml_config_test;

#[cfg(test)]
#[path = "../../test/config/cmd_timeout.rs"]
mod cmd_timeout_test;
