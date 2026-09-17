use std::path::PathBuf;

use crate::utils::xdg;
use crate::utils::xdg::XdgError;

use super::PluginConfig;

#[derive(Debug)]
pub enum TomlConfigError {
    InvalidApiKey(String),
    NotFound(PathBuf),
    Parse(toml::de::Error),
    Io(std::io::Error),
    Xdg(XdgError),
}

impl std::fmt::Display for TomlConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidApiKey(message) => write!(f, "{message}"),
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
    let dir = xdg::config_dir().map_err(TomlConfigError::Xdg)?;
    Ok(dir.join("nu-agent").join("config.toml"))
}

/// Load PluginConfig from config.toml.
/// Returns `PluginConfig::default()` if the file doesn't exist (not an error).
/// Returns an error only if the file exists but can't be read or parsed.
pub fn load() -> Result<PluginConfig, TomlConfigError> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(PluginConfig::default());
    }
    let contents = std::fs::read_to_string(&path).map_err(TomlConfigError::Io)?;
    let config: PluginConfig = toml::from_str(&contents).map_err(TomlConfigError::Parse)?;
    validate_api_keys(&config)?;
    Ok(config)
}

/// Reject raw API key literals in config.toml.
///
/// Every `providers.<name>.api_key` value must be a `store:` or `env:`
/// reference. A raw literal would put a plaintext secret on disk, so it is a
/// hard error.
fn validate_api_keys(config: &PluginConfig) -> Result<(), TomlConfigError> {
    for (name, provider) in &config.providers {
        let Some(api_key) = &provider.api_key else {
            continue;
        };
        if api_key.starts_with("store:") || api_key.starts_with("env:") {
            continue;
        }
        return Err(TomlConfigError::InvalidApiKey(format!(
            "api_key in [providers.{name}] must be a store: or env: reference, not a raw value. Use: agent provider auth login {name}"
        )));
    }
    Ok(())
}

#[cfg(test)]
#[path = "toml_config_test.rs"]
mod toml_config_test;
