//! Shared configuration loading and model resolution.
//!
//! Both the Nushell plugin path (`nu-agent`) and the ACP path (`agnt-acp`)
//! load `config.toml`, resolve a model role, and build a runtime `Config`. This
//! module owns that shared pipeline so the two paths cannot drift.

// region:    --- Modules

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::protocol::preamble::{
    PreambleDefaults, UserPreambleInput, classify_model_family, resolve_preamble,
};
use crate::utils::env_map::{EnvMap, process_env};

use super::dotenv;
use super::models_cache::{ModelsCache, ModelsCacheError};
use super::paths::{Paths, PathsError};
use super::toml_config::{self, TomlConfigError};
use super::types::{Config, ModelRoleConfig, PluginConfig};
use super::vault::{Vault, VaultError};

// endregion: --- Modules

// region:    --- Types

/// Errors from the shared config loading and resolution pipeline.
#[derive(Debug)]
pub enum ConfigLoadError {
    /// config.toml does not exist at the given path.
    NotFound(PathBuf),
    /// config.toml exists but could not be read or parsed.
    Toml(TomlConfigError),
    /// The requested model role is not configured.
    UnknownRole(String),
    /// No `models.default` role is configured.
    NoDefaultModel,
    /// The model role could not be resolved to a runtime config.
    Resolve(String),
    /// The vault backend could not be selected.
    Vault(VaultError),
    /// The application paths could not be resolved.
    Paths(PathsError),
}

impl std::fmt::Display for ConfigLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(path) => write!(f, "config not found at {}", path.display()),
            Self::Toml(e) => write!(f, "{e}"),
            Self::UnknownRole(role) => write!(f, "unknown model role: '{role}'"),
            Self::NoDefaultModel => write!(f, "no model configured"),
            Self::Resolve(msg) => write!(f, "failed to resolve model: {msg}"),
            Self::Vault(e) => write!(f, "vault backend unavailable: {e}"),
            Self::Paths(e) => write!(f, "failed to resolve application paths: {e}"),
        }
    }
}

impl std::error::Error for ConfigLoadError {}

// endregion: --- Types

// region:    --- Public Functions

/// Load config.toml, .env files, vault, and models cache, then resolve a model
/// role into a runtime `Config`.
///
/// `model_override` selects the model role:
/// - `None` — the `models.default` role
/// - `Some("provider/model")` — a literal model spec
/// - `Some("label")` — a role label from `models`
///
/// CLI flag overrides are not applied here. The plugin path applies them after
/// this call; the ACP path has no CLI flags.
///
/// # Errors
/// - [`ConfigLoadError::NotFound`] when `config_path` does not exist
/// - [`ConfigLoadError::Toml`] when config.toml cannot be read or parsed
/// - [`ConfigLoadError::UnknownRole`] / [`ConfigLoadError::NoDefaultModel`]
///   when the selected role is not configured
/// - [`ConfigLoadError::Resolve`] when the role cannot be resolved
/// - [`ConfigLoadError::Vault`] when the vault backend cannot be selected
pub fn load_and_resolve_config(
    config_path: &Path,
    model_override: Option<&str>,
) -> Result<(Config, PluginConfig), ConfigLoadError> {
    if !config_path.exists() {
        return Err(ConfigLoadError::NotFound(config_path.to_path_buf()));
    }
    let mut plugin_config = toml_config::load_from(config_path).map_err(ConfigLoadError::Toml)?;
    // The `[paths]` values in the config decide where the vault and the models
    // cache live, so the paths resolve before either is read.
    let config_dir = config_path.parent().unwrap_or_else(|| Path::new("."));
    let paths =
        Paths::resolve(plugin_config.paths.as_ref(), config_dir).map_err(ConfigLoadError::Paths)?;
    // Load .env files before anything reads the environment. User-level and
    // project-level values only fill variables that are not already set.
    dotenv::load_env_files();
    // Probe keychain, then file, then env-only. The first available wins.
    plugin_config.vault = Some(Arc::new(
        Vault::auto_detect(&paths.secrets_path).map_err(ConfigLoadError::Vault)?,
    ));
    match ModelsCache::load_from(&paths.models_cache_path) {
        Ok(cache) => plugin_config.models_cache = Some(cache),
        Err(ModelsCacheError::NotFound(_)) => {}
        Err(e) => log::warn!("Failed to load models cache: {e}"),
    }
    let config = resolve_role_config(&plugin_config, model_override, &process_env())?;
    Ok((config, plugin_config))
}

/// Resolve a model role against an already-loaded `PluginConfig`.
///
/// Same role selection as [`load_and_resolve_config`], but the caller owns the
/// `PluginConfig` and the environment map.
///
/// # Errors
/// - [`ConfigLoadError::UnknownRole`] / [`ConfigLoadError::NoDefaultModel`]
///   when the selected role is not configured
/// - [`ConfigLoadError::Resolve`] when the role cannot be resolved
pub fn resolve_role_config(
    plugin_config: &PluginConfig,
    model_override: Option<&str>,
    env: &EnvMap,
) -> Result<Config, ConfigLoadError> {
    let role_config = select_role(plugin_config, model_override)?;
    let mut config = plugin_config
        .resolve_model_with(&role_config, env)
        .map_err(ConfigLoadError::Resolve)?;
    if let Some((provider_name, model_name)) = role_config.model.split_once('/') {
        config.preamble = resolve_preamble_for_model(plugin_config, provider_name, model_name);
    }
    Ok(config)
}

/// Resolve the preamble for a specific provider/model pair.
///
/// Looks up the provider config and model config from `plugin_config` and
/// resolves the preamble using the canonical `resolve_preamble` function.
/// Returns `None` if the provider is not found in the plugin config.
pub fn resolve_preamble_for_model(
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

// endregion: --- Public Functions

// region:    --- Support

/// Select the model role config for `model_override`.
fn select_role(
    plugin_config: &PluginConfig,
    model_override: Option<&str>,
) -> Result<ModelRoleConfig, ConfigLoadError> {
    match model_override {
        Some(spec) if spec.contains('/') => Ok(ModelRoleConfig {
            model: spec.to_string(),
            ..Default::default()
        }),
        Some(label) => plugin_config
            .models
            .get(label)
            .cloned()
            .ok_or_else(|| ConfigLoadError::UnknownRole(label.to_string())),
        None => plugin_config
            .models
            .get("default")
            .cloned()
            .ok_or(ConfigLoadError::NoDefaultModel),
    }
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "../../test/config/loader.rs"]
mod loader_test;

#[cfg(all(test, feature = "integration"))]
#[path = "../../test/config/loader_integration.rs"]
mod loader_integration_test;

// endregion: --- Tests
