//! clap-based permissions resolution for the CLI.

use agnt_core::config::PluginConfig;
use agnt_core::tools::authz::{PermissionsConfig, PermissionsOverlay};

// region:    --- Functions

/// Build the base and effective permission configs.
///
/// The CLI has no `--permissions` flag, so the chain is `base → agent_overlay`.
/// Returns `(base, effective, cli_overlay, startup_summary)`; `cli_overlay` is
/// always `None` and is kept for parity with the plugin's runtime parameters.
pub fn resolve_effective_permissions_from_clap(
    plugin_config: &PluginConfig,
    agent_overlay: Option<&PermissionsOverlay>,
    interactive: bool,
) -> (
    PermissionsConfig,
    PermissionsConfig,
    Option<PermissionsOverlay>,
    String,
) {
    let (base, effective, startup_message) = agnt_core::tools::authz::resolve_effective_permissions(
        plugin_config,
        agent_overlay,
        None,
        interactive,
    );
    (base, effective, None, startup_message)
}

// endregion: --- Functions
