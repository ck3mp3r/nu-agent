//! `resolve_hooks`: script path resolution and the timeout default.

use super::*;

#[test]
fn resolve_hooks_joins_a_relative_script_to_the_config_dir() -> Result<()> {
    // -- Setup & Fixtures
    let config = PluginConfig {
        hooks: vec![HookConfig {
            name: "audit".to_string(),
            events: vec![HookEvent::TurnStarted],
            script: "hooks/audit.nu".to_string(),
            timeout_ms: None,
        }],
        ..PluginConfig::default()
    };

    // -- Exec
    let resolved = resolve_hooks(&config, std::path::Path::new("/etc/nu-agent"))
        .map_err(|e| format!("resolution should succeed: {e:?}"))?;

    // -- Check
    assert_eq!(resolved.len(), 1);
    assert_eq!(
        resolved[0].script,
        std::path::PathBuf::from("/etc/nu-agent/hooks/audit.nu"),
        "a relative script resolves against the config directory"
    );
    assert_eq!(
        resolved[0].timeout,
        std::time::Duration::from_millis(10_000),
        "an absent timeout_ms falls back to the 10s default"
    );
    Ok(())
}

#[test]
fn resolve_hooks_keeps_an_absolute_script() -> Result<()> {
    // -- Setup & Fixtures
    let config = PluginConfig {
        hooks: vec![HookConfig {
            name: "audit".to_string(),
            events: vec![HookEvent::TurnCompleted],
            script: "/opt/hooks/audit.nu".to_string(),
            timeout_ms: Some(500),
        }],
        ..PluginConfig::default()
    };

    // -- Exec
    let resolved = resolve_hooks(&config, std::path::Path::new("/etc/nu-agent"))
        .map_err(|e| format!("resolution should succeed: {e:?}"))?;

    // -- Check
    assert_eq!(
        resolved[0].script,
        std::path::PathBuf::from("/opt/hooks/audit.nu"),
        "an absolute script stays as written"
    );
    assert_eq!(resolved[0].timeout, std::time::Duration::from_millis(500));
    Ok(())
}

#[test]
fn resolve_hooks_rejects_an_empty_name() -> Result<()> {
    // -- Setup & Fixtures
    let config = PluginConfig {
        hooks: vec![HookConfig {
            name: String::new(),
            events: vec![HookEvent::TurnStarted],
            script: "audit.nu".to_string(),
            timeout_ms: None,
        }],
        ..PluginConfig::default()
    };

    // -- Exec
    let err = match resolve_hooks(&config, std::path::Path::new("/etc/nu-agent")) {
        Ok(_) => return Err("an empty hook name must not resolve".into()),
        Err(e) => e,
    };

    // -- Check
    assert!(
        matches!(err, TomlConfigError::Hook(_)),
        "expected a Hook error, got {err:?}"
    );
    Ok(())
}
