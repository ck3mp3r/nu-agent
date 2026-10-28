//! `agent config` subcommand dispatch.

use crate::ConfigCommand;
use crate::ConfigInitArgs;
use crate::error::{Error, Result};

// region:    --- Functions

/// Dispatch an `agent config` subcommand.
///
/// The config file path is resolved once here, from the [`Paths`] the
/// environment ladder selects, and handed to the subcommand.
///
/// [`Paths`]: agnt_core::config::Paths
pub fn run_config_command(command: &ConfigCommand) -> Result<()> {
    let paths = agnt_core::config::Paths::resolve_for_process(None)
        .map_err(|e| Error::Custom(format!("Cannot determine config path: {e}")))?;
    match command {
        ConfigCommand::Init(args) => run_config_init(args, &paths.config_path),
    }
}

/// Generate a starter config.toml from the current environment variables.
///
/// `path` is the resolved config file path; this function never re-derives it.
fn run_config_init(args: &ConfigInitArgs, path: &std::path::Path) -> Result<()> {
    if path.exists() && !args.force {
        return Err(Error::Custom(format!(
            "config.toml already exists at {}. Use --force to overwrite.",
            path.display()
        )));
    }

    let content = generate_config_content();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Error::Custom(format!("Failed to create config dir: {e}")))?;
    }
    std::fs::write(path, &content)
        .map_err(|e| Error::Custom(format!("Failed to write config: {e}")))?;

    println!(
        "Created config.toml at {}\n\nEdit it to set your model, then run:\n  agent provider auth login <provider>  (to store API keys)\n  agent models sync              (to fetch model specs)",
        path.display()
    );
    Ok(())
}

// endregion: --- Functions

// region:    --- Support

/// The starter config template, shared with the plugin's `agent config init`.
///
/// The plugin owns the canonical template; the CLI includes it directly so the
/// two commands cannot drift apart.
const TEMPLATE: &str = include_str!("../../../../agnt-plugin/src/command/config/template.toml");

fn generate_config_content() -> String {
    let env = agnt_core::utils::env_map::process_env();
    let env_provider = env.get("AGENT_PROVIDER").cloned().unwrap_or_default();
    let env_model = env.get("AGENT_MODEL").cloned().unwrap_or_default();

    let active_model = if !env_provider.is_empty() && !env_model.is_empty() {
        format!("[models.default]\nmodel = \"{env_provider}/{env_model}\"")
    } else {
        "# Edit this to set your default model:\n# [models.default]\n# model = \"ollama-cloud/glm-5.2\"".to_string()
    };

    TEMPLATE.replace("{{ACTIVE_MODEL}}", &active_model)
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "../../../test/dispatch/utility/config.rs"]
mod config_test;

// endregion: --- Tests
