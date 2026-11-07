//! Agent persona configuration: which built-in personas are available and
//! which one is the default.

use serde::{Deserialize, Serialize};

/// Configuration for agent personas (planner/maker).
///
/// Controls which built-in personas are available and which is the default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentsConfig {
    /// Whether the planner persona is enabled
    #[serde(default = "default_true")]
    pub planner_enabled: bool,
    /// Whether the maker persona is enabled
    #[serde(default = "default_true")]
    pub maker_enabled: bool,
    /// Default persona at startup (e.g., "planner" or "maker")
    #[serde(default)]
    pub default: String,
    /// Fallback persona name (an .agents/*.md file) when default is disabled
    pub fallback: Option<String>,
}

fn default_true() -> bool {
    true
}

impl Default for AgentsConfig {
    fn default() -> Self {
        Self {
            planner_enabled: true,
            maker_enabled: true,
            default: "planner".to_string(),
            fallback: None,
        }
    }
}

impl AgentsConfig {
    /// Whether a built-in persona is enabled by this config.
    ///
    /// Non-built-in personas are always enabled.
    pub fn is_builtin_enabled(&self, name: &str) -> bool {
        use crate::protocol::persona::builtins;
        match name {
            n if n == builtins::BUILTIN_PLANNER_NAME => self.planner_enabled,
            n if n == builtins::BUILTIN_MAKER_NAME => self.maker_enabled,
            _ => true,
        }
    }

    /// Resolve the default agent, falling back when the default is disabled.
    ///
    /// Returns `Err` when the default is a disabled built-in and no fallback is set.
    pub fn resolve_default_agent(&self) -> Result<Option<String>, String> {
        use crate::protocol::persona::builtins;
        let default = &self.default;
        if builtins::is_builtin_persona(default) && !self.is_builtin_enabled(default) {
            match &self.fallback {
                Some(fallback) => Ok(Some(fallback.clone())),
                None => Err(format!(
                    "Default agent '{}' is disabled and no fallback configured. \
                     Set `agents.fallback` in config or enable '{}'.",
                    default, default
                )),
            }
        } else {
            Ok(Some(default.clone()))
        }
    }
}
