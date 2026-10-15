use super::*;

use agnt_core::tools::authz::PermissionAction;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn test_dispatch_permissions_no_overlay_reports_inactive() -> Result<()> {
    // -- Setup & Fixtures
    let plugin_config = PluginConfig::default();

    // -- Exec
    let (base, effective, cli_overlay, summary) =
        resolve_effective_permissions_from_clap(&plugin_config, None, true);

    // -- Check
    assert!(cli_overlay.is_none());
    assert!(
        summary.contains("overlay_active=false"),
        "summary={summary}"
    );
    assert_eq!(base.summary().global, effective.summary().global);
    Ok(())
}

#[test]
fn test_dispatch_permissions_agent_overlay_reports_active() -> Result<()> {
    // -- Setup & Fixtures
    let plugin_config = PluginConfig::default();
    let overlay = PermissionsOverlay::default();

    // -- Exec
    let (_base, _effective, cli_overlay, summary) =
        resolve_effective_permissions_from_clap(&plugin_config, Some(&overlay), false);

    // -- Check
    assert!(cli_overlay.is_none());
    assert!(summary.contains("overlay_active=true"), "summary={summary}");
    Ok(())
}

#[test]
fn test_dispatch_permissions_interactive_selects_global_action() -> Result<()> {
    // -- Setup & Fixtures
    let plugin_config = PluginConfig::default();

    // -- Exec
    let (interactive_base, _eff, _cli, _summary) =
        resolve_effective_permissions_from_clap(&plugin_config, None, true);
    let (non_interactive_base, _eff, _cli, _summary) =
        resolve_effective_permissions_from_clap(&plugin_config, None, false);

    // -- Check
    assert_eq!(interactive_base.summary().global, PermissionAction::Ask);
    assert_eq!(
        non_interactive_base.summary().global,
        PermissionAction::Deny
    );
    Ok(())
}
