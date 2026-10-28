//! Resolved application paths.
//!
//! Every filesystem path nu-agent touches is resolved once, at startup, by
//! [`Paths::resolve_for_process`]. Each tree follows one 4-level ladder, most
//! specific level first:
//!
//! 1. `NU_AGENT_<X>_HOME` environment variable
//! 2. `[paths].<x>_home` in config.toml
//! 3. `XDG_<X>_HOME` environment variable
//! 4. the XDG default under `$HOME`
//!
//! An override re-roots its whole tree and keeps the `{APP_DIR_NAME}/<tail>`
//! substructure. The config tree has no `[paths]` level: the file that would
//! hold it is the file the ladder has to find first.
//!
//! Every `NU_AGENT_*_HOME` value and every `[paths]` value must be absolute or
//! start with `~`; a relative value is an error. A leading `~` expands to
//! `HOME`, an empty value counts as absent, and the `-dev` split of
//! [`APP_DIR_NAME`] applies to every derived path.
//!
//! All resolution policy lives here: no other module re-derives a base, joins
//! an artifact filename, or calls `xdg::*` for an application path. The fields
//! of [`Paths`] are final values; directories are created on demand by the
//! consumer that first writes there.

// region:    --- Modules

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::utils::env_map::{EnvMap, process_env};
use crate::utils::paths::APP_DIR_NAME;
use crate::utils::xdg::{self, XdgError};

use super::copilot_tokens::copilot_dir_name;

// endregion: --- Modules

// region:    --- Constants

/// Config file name inside the application config directory.
const CONFIG_FILE: &str = "config.toml";
/// User-level dotenv file name inside the application config directory.
const DOTENV_FILE: &str = ".env";
/// Persona directory name inside the application config directory.
const AGENTS_DIR: &str = "agents";
/// rig-memory store file name inside the application cache directory.
pub const STORE_FILE: &str = "rig-sessions.db";
/// Log directory name inside the application state directory.
const LOG_DIR: &str = "logs";
/// Log file name inside the log directory.
const LOG_FILE: &str = "agent.log";
/// Tool audit log file name inside the application data directory.
const AUDIT_LOG_FILE: &str = "tool_audit.log";
/// Secrets file name inside the application data directory.
const SECRETS_FILE: &str = "secrets.json";
/// models.dev cache file name inside the application data directory.
const MODELS_FILE: &str = "models.json";
/// Theme preference file name inside the application data directory.
const THEME_FILE: &str = "theme.json";
/// Grammar cache directory name inside the cache base.
///
/// The tree-sitter ecosystem convention: the `tree-sitter` CLI keeps its
/// compiled grammars in `{cache base}/tree-sitter`, shared with this tool. The
/// directory therefore keeps no `APP_DIR_NAME` substructure and no `-dev` split.
const TREE_SITTER_DIR: &str = "tree-sitter";

/// Environment override re-rooting the config tree.
pub const ENV_CONFIG_HOME: &str = "NU_AGENT_CONFIG_HOME";
/// Environment override re-rooting the tool audit, secrets, models cache, and
/// theme tree.
pub const ENV_DATA_HOME: &str = "NU_AGENT_DATA_HOME";
/// Environment override re-rooting the rig-memory store tree.
pub const ENV_CACHE_HOME: &str = "NU_AGENT_CACHE_HOME";
/// Environment override re-rooting the log tree.
pub const ENV_STATE_HOME: &str = "NU_AGENT_STATE_HOME";

// endregion: --- Constants

// region:    --- Types

/// The `[paths]` block of config.toml.
///
/// Each field is a base-dir override: it re-roots its tree and keeps the
/// `APP_DIR_NAME` substructure. A relative value is rejected at parse time,
/// because an override must name a base that stands on its own.
///
/// The config tree has no field here: the config file that would hold it is the
/// file the ladder has to find first.
///
/// An unknown key is a hard error, not a silent no-op: a key this struct does
/// not name stops working the moment the field set changes, so a stale key must
/// surface at parse time instead of quietly overriding nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathsConfig {
    /// Base directory that re-roots the data tree.
    pub data_home: Option<String>,
    /// Base directory that re-roots the cache tree.
    pub cache_home: Option<String>,
    /// Base directory that re-roots the state tree.
    pub state_home: Option<String>,
}

/// Every application path, resolved once at startup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// Final path of config.toml.
    pub config_path: PathBuf,
    /// Final path of the user-level `.env` file.
    pub dotenv_path: PathBuf,
    /// Final path of the persona directory.
    pub agents_dir: PathBuf,
    /// Final path of the Copilot credential directory.
    pub copilot_token_dir: PathBuf,
    /// Final path of the rig-memory store file.
    pub store_path: PathBuf,
    /// Final path of the log directory.
    pub log_dir: PathBuf,
    /// Final path of the log file.
    pub log_path: PathBuf,
    /// Final path of the tool audit log file.
    pub audit_log_path: PathBuf,
    /// Final path of the secrets file.
    pub secrets_path: PathBuf,
    /// Final path of the models.dev cache file.
    pub models_cache_path: PathBuf,
    /// Final path of the theme preference file.
    pub theme_path: PathBuf,
    /// Final path of the tree-sitter compiled-grammar cache directory.
    ///
    /// Shared with the `tree-sitter` CLI, so it keeps no `APP_DIR_NAME`
    /// substructure and no `-dev` split.
    pub grammar_cache_dir: PathBuf,
}

/// Errors that can occur while resolving the application paths.
#[derive(Debug)]
pub enum PathsError {
    /// `HOME` is unset but a `~` value needs expanding.
    HomeNotFound,
    /// An override names a relative base instead of an absolute one.
    RelativeValue {
        /// The environment variable or `[paths]` field that holds the value.
        label: String,
        /// The rejected value, as written.
        value: String,
    },
    /// An XDG base directory could not be resolved.
    Xdg(XdgError),
}

// endregion: --- Types

// region:    --- Froms

impl From<XdgError> for PathsError {
    fn from(error: XdgError) -> Self {
        Self::Xdg(error)
    }
}

// endregion: --- Froms

// region:    --- Support Impls

impl std::fmt::Display for PathsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HomeNotFound => write!(f, "HOME environment variable not set"),
            Self::RelativeValue { label, value } => write!(
                f,
                "{label} must be an absolute path or start with '~', not the relative path '{value}'"
            ),
            Self::Xdg(error) => write!(f, "XDG error: {error}"),
        }
    }
}

impl std::error::Error for PathsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::HomeNotFound | Self::RelativeValue { .. } => None,
            Self::Xdg(error) => Some(error),
        }
    }
}

// endregion: --- Support Impls

// region:    --- Paths

impl Paths {
    // --- Constructors ---

    /// Resolve every application path from config and the process environment.
    ///
    /// `config_dir` is the directory holding the config.toml that was loaded:
    /// the config tree hangs off it, and every other tree follows its own
    /// ladder. A caller that has not found the config file yet calls
    /// [`Self::resolve_for_process`] instead.
    pub fn resolve(config: Option<&PathsConfig>, config_dir: &Path) -> Result<Self, PathsError> {
        Self::resolve_with(config, config_dir, &process_env())
    }

    /// Resolve every application path from config and the process environment.
    ///
    /// The config dir comes from the config file the environment selects, so a
    /// caller never derives it.
    pub fn resolve_for_process(config: Option<&PathsConfig>) -> Result<Self, PathsError> {
        Self::resolve_for_process_with(config, &process_env())
    }

    /// Resolve every application path from injected inputs, taking the config
    /// dir from the config file the environment selects.
    ///
    /// As [`Self::resolve_for_process`], with `env` injected so a test never
    /// touches the process environment.
    pub fn resolve_for_process_with(
        config: Option<&PathsConfig>,
        env: &EnvMap,
    ) -> Result<Self, PathsError> {
        let bootstrap = Self::bootstrap_config_path_with(env)?;
        Self::resolve_with(config, &parent_dir(&bootstrap), env)
    }

    /// Resolve every application path from config.toml, falling back to the
    /// derived defaults when the file is absent or unreadable.
    ///
    /// Command entry points that load no model config (provider and MCP auth)
    /// call this so they act on the same files the runtime reads. A broken
    /// config counts as absent here: the command still runs, and the failure
    /// surfaces when the runtime loads the config.
    pub fn resolve_from_config_or_default() -> Result<Self, PathsError> {
        let config = super::toml_config::load().unwrap_or_default();
        Self::resolve_for_process(config.paths.as_ref())
    }

    // --- Accessors ---

    /// The directory holding config.toml.
    ///
    /// This is the anchor a relative hook script resolves against.
    pub fn config_dir(&self) -> &Path {
        self.config_path.parent().unwrap_or_else(|| Path::new("."))
    }

    /// The config file path the process bootstrap follows.
    ///
    /// This is the one discovery rule: `NU_AGENT_CONFIG_HOME` wins, else
    /// `{XDG config base}/{APP_DIR_NAME}/config.toml`. The config tree has no
    /// `[paths]` level, so discovery and [`Self::resolve`] derive the config
    /// tree from the same ladder and the two can never anchor apart.
    ///
    /// # Errors
    ///
    /// [`PathsError::HomeNotFound`] when no level of the ladder resolves, and
    /// [`PathsError::RelativeValue`] when the override is relative.
    pub fn bootstrap_config_path() -> Result<PathBuf, PathsError> {
        Self::bootstrap_config_path_with(&process_env())
    }

    /// The config file path the process bootstrap follows, from injected inputs.
    ///
    /// As [`Self::bootstrap_config_path`], with `env` injected.
    pub fn bootstrap_config_path_with(env: &EnvMap) -> Result<PathBuf, PathsError> {
        let base = match non_empty(env, ENV_CONFIG_HOME) {
            Some(raw) => absolute_dir(raw, ENV_CONFIG_HOME, env)?,
            None => xdg::config_dir_with(env).map_err(PathsError::Xdg)?,
        };
        Ok(base.join(APP_DIR_NAME).join(CONFIG_FILE))
    }

    /// Resolve every application path from injected inputs.
    ///
    /// `config_dir` is the directory holding the loaded config.toml and `env`
    /// supplies the overrides plus the XDG bases. Injection keeps resolution
    /// testable without touching the process environment.
    pub fn resolve_with(
        config: Option<&PathsConfig>,
        config_dir: &Path,
        env: &EnvMap,
    ) -> Result<Self, PathsError> {
        let config = config.cloned().unwrap_or_default();

        let config_app_dir = config_dir.to_path_buf();
        let config_path = config_app_dir.join(CONFIG_FILE);

        let data_app_dir = app_base(
            ENV_DATA_HOME,
            "[paths].data_home",
            config.data_home.as_deref(),
            env,
            xdg::data_dir_with,
        )?;

        let cache_base = tree_base(
            ENV_CACHE_HOME,
            "[paths].cache_home",
            config.cache_home.as_deref(),
            env,
            xdg::cache_dir_with,
        )?;
        let store_path = cache_base.join(APP_DIR_NAME).join(STORE_FILE);
        let grammar_cache_dir = cache_base.join(TREE_SITTER_DIR);

        let log_path = app_base(
            ENV_STATE_HOME,
            "[paths].state_home",
            config.state_home.as_deref(),
            env,
            xdg::state_dir_with,
        )?
        .join(LOG_DIR)
        .join(LOG_FILE);
        let log_dir = parent_dir(&log_path);

        Ok(Self {
            config_path,
            dotenv_path: config_app_dir.join(DOTENV_FILE),
            agents_dir: config_app_dir.join(AGENTS_DIR),
            copilot_token_dir: config_app_dir.join(copilot_dir_name()),
            store_path,
            log_dir,
            log_path,
            audit_log_path: data_app_dir.join(AUDIT_LOG_FILE),
            secrets_path: data_app_dir.join(SECRETS_FILE),
            models_cache_path: data_app_dir.join(MODELS_FILE),
            theme_path: data_app_dir.join(THEME_FILE),
            grammar_cache_dir,
        })
    }
}

// endregion: --- Paths

// region:    --- Support

/// Resolve one tree's base: `env_key`, then the `field` value from `[paths]`,
/// then the XDG base.
///
/// An empty value counts as absent at both override levels.
fn tree_base(
    env_key: &str,
    field: &str,
    config_value: Option<&str>,
    env: &EnvMap,
    xdg_base: impl FnOnce(&EnvMap) -> Result<PathBuf, XdgError>,
) -> Result<PathBuf, PathsError> {
    if let Some(raw) = non_empty(env, env_key) {
        return absolute_dir(raw, env_key, env);
    }
    if let Some(raw) = config_value.filter(|value| !value.is_empty()) {
        return absolute_dir(raw, field, env);
    }
    xdg_base(env).map_err(PathsError::Xdg)
}

/// Resolve one tree's base onto `{base}/{APP_DIR_NAME}`.
fn app_base(
    env_key: &str,
    field: &str,
    config_value: Option<&str>,
    env: &EnvMap,
    xdg_base: impl FnOnce(&EnvMap) -> Result<PathBuf, XdgError>,
) -> Result<PathBuf, PathsError> {
    Ok(tree_base(env_key, field, config_value, env, xdg_base)?.join(APP_DIR_NAME))
}

/// Resolve an override to an absolute directory, rejecting a relative value.
///
/// An override re-roots its whole tree, so it must name a base that stands on
/// its own; `label` names the environment variable or `[paths]` field in the
/// error.
fn absolute_dir(raw: &str, label: &str, env: &EnvMap) -> Result<PathBuf, PathsError> {
    let path = expand_tilde(raw, env)?;
    if path.is_absolute() {
        return Ok(path);
    }
    Err(PathsError::RelativeValue {
        label: label.to_string(),
        value: raw.to_string(),
    })
}

/// Expand a leading `~` to `HOME`. Any other `~` usage stays as written.
fn expand_tilde(raw: &str, env: &EnvMap) -> Result<PathBuf, PathsError> {
    if raw != "~" && !raw.starts_with("~/") {
        return Ok(PathBuf::from(raw));
    }
    let home = non_empty(env, "HOME").ok_or(PathsError::HomeNotFound)?;
    if raw == "~" {
        return Ok(PathBuf::from(home));
    }
    Ok(PathBuf::from(home).join(&raw[2..]))
}

/// The parent directory of `path`, or `.` when it has none.
fn parent_dir(path: &Path) -> PathBuf {
    path.parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

/// Look up `key`, treating an empty value as absent.
fn non_empty<'a>(env: &'a EnvMap, key: &str) -> Option<&'a str> {
    env.get(key).map(String::as_str).filter(|s| !s.is_empty())
}

// endregion: --- Support
