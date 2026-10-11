use nu_plugin::EvaluatedCall;
use nu_protocol::{LabeledError, Value};

use agnt_core::tools::authz::{PermissionsConfig, PermissionsOverlay};

pub(super) fn resolve_effective_permissions_config(
    call: &EvaluatedCall,
    plugin_config: &agnt_core::config::PluginConfig,
    agent_overlay: Option<&PermissionsOverlay>,
    interactive: bool,
) -> Result<
    (
        PermissionsConfig,          // base — raw plugin config, no overlays
        PermissionsConfig,          // effective — base + agent_overlay + CLI
        Option<PermissionsOverlay>, // cli_overlay — parsed --permissions flag
        String,                     // startup summary message
    ),
    LabeledError,
> {
    let cli_permissions: Option<Value> = call.get_flag("permissions").ok().flatten();

    // Parse CLI overlay separately so it can be re-applied during persona switches
    let cli_overlay: Option<PermissionsOverlay> = cli_permissions
        .as_ref()
        .map(|value| {
            PermissionsOverlay::parse_from_cli_value(value).map_err(|msg| {
                LabeledError::new("Invalid --permissions value").with_label(msg, value.span())
            })
        })
        .transpose()?;

    // Build permission chain: base → agent_overlay → CLI
    // CLI always wins (highest precedence)
    let (base, effective, startup_message) = agnt_core::tools::authz::resolve_effective_permissions(
        plugin_config,
        agent_overlay,
        cli_overlay.as_ref(),
        interactive,
    );

    Ok((base, effective, cli_overlay, startup_message))
}
