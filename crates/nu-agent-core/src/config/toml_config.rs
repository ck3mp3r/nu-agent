use std::path::PathBuf;

use crate::utils::xdg;
use crate::utils::xdg::XdgError;

use super::PluginConfig;

#[derive(Debug)]
pub enum TomlConfigError {
    NotFound(PathBuf),
    Parse(toml::de::Error),
    Io(std::io::Error),
    Xdg(XdgError),
}

impl std::fmt::Display for TomlConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
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
    Ok(config)
}

#[cfg(test)]
#[path = "toml_config_test.rs"]
mod toml_config_test;
