use nu_plugin::EvaluatedCall;
use nu_protocol::{LabeledError, Value};

use nu_agent_core::tools::authz::{PermissionsConfig, PermissionsOverlay};
pub(super) fn is_builtin_enabled(name: &str, config: &nu_agent_core::config::AgentsConfig) -> bool {
    use nu_agent_core::protocol::persona::builtins;
    match name {
        n if n == builtins::BUILTIN_PLANNER_NAME => config.planner_enabled,
        n if n == builtins::BUILTIN_MAKER_NAME => config.maker_enabled,
        _ => true, // non-builtins are always "enabled"
    }
}

pub(super) fn resolve_default_agent(
    config: &nu_agent_core::config::AgentsConfig,
) -> Result<Option<String>, LabeledError> {
    use nu_agent_core::protocol::persona::builtins;
    let default = &config.default;
    if builtins::is_builtin_persona(default) && !is_builtin_enabled(default, config) {
        // Default is disabled — try fallback
        match &config.fallback {
            Some(fallback) => Ok(Some(fallback.clone())),
            None => Err(LabeledError::new(format!(
                "Default agent '{}' is disabled and no fallback configured. \
                 Set `agents.fallback` in config or enable '{}'.",
                default, default
            ))),
        }
    } else {
        Ok(Some(default.clone()))
    }
}

pub(super) fn resolve_effective_permissions_config(
    call: &EvaluatedCall,
    plugin_config: Option<&Value>,
    agent_overlay: Option<&PermissionsOverlay>,
    interactive: bool,
) -> Result<(PermissionsConfig, String), LabeledError> {
    let base = PermissionsConfig::parse_from_plugin_config(plugin_config, interactive);
    let cli_permissions: Option<Value> = call.get_flag("permissions").ok().flatten();

    // Build permission chain: base → agent_overlay → CLI
    // CLI always wins (highest precedence)
    let mut effective = base;

    if let Some(overlay) = agent_overlay {
        effective = effective.with_overlay(overlay);
    }

    if let Some(value) = cli_permissions.as_ref() {
        let overlay = PermissionsOverlay::parse_from_cli_value(value).map_err(|msg| {
            LabeledError::new("Invalid --permissions value").with_label(msg, value.span())
        })?;
        effective = effective.with_overlay(&overlay);
    }

    let summary = effective.summary();
    let overlay_active = agent_overlay.is_some() || cli_permissions.is_some();
    let startup_message = format!(
        "permissions policy: overlay_active={} global={} tool_rules={} nu__run.command_rules={}",
        overlay_active,
        summary.global.as_str(),
        summary.tool_rule_count,
        summary.nested_field_rule_count,
    );

    Ok((effective, startup_message))
}
