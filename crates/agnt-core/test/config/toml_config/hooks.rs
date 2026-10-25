//! Parsing and validation of the `[[hooks]]` block.

use super::*;
use crate::config::{HookConfig, HookEvent, HookEventClass, resolve_hooks};

// region:    --- parsing

#[test]
fn load_parses_a_hooks_block() -> Result<()> {
    with_xdg_config_home(|dir| {
        let path = write_config(
            dir,
            r#"
[[hooks]]
name = "audit"
events = ["turn_started", "turn_completed"]
script = "hooks/audit.nu"
timeout_ms = 2500
"#,
        )?;

        let config = load_from(&path).map_err(|e| format!("load should succeed: {e:?}"))?;

        assert_eq!(config.hooks.len(), 1, "one [[hooks]] entry must parse");
        let hook = &config.hooks[0];
        assert_eq!(hook.name, "audit");
        assert_eq!(
            hook.events,
            vec![HookEvent::TurnStarted, HookEvent::TurnCompleted]
        );
        assert_eq!(hook.script, "hooks/audit.nu");
        assert_eq!(hook.timeout_ms, Some(2500));
        Ok(())
    })
}

#[test]
fn load_parses_multiple_hooks() -> Result<()> {
    with_xdg_config_home(|dir| {
        let path = write_config(
            dir,
            r#"
[[hooks]]
name = "audit"
events = ["turn_started"]
script = "audit.nu"

[[hooks]]
name = "notify"
events = ["agent_started", "agent_shutdown"]
script = "notify.nu"
"#,
        )?;

        let config = load_from(&path).map_err(|e| format!("load should succeed: {e:?}"))?;

        assert_eq!(config.hooks.len(), 2, "both entries must parse");
        assert_eq!(config.hooks[1].name, "notify");
        assert_eq!(
            config.hooks[1].events,
            vec![HookEvent::AgentStarted, HookEvent::AgentShutdown]
        );
        assert_eq!(config.hooks[1].timeout_ms, None);
        Ok(())
    })
}

#[test]
fn load_parses_without_a_hooks_block() -> Result<()> {
    with_xdg_config_home(|dir| {
        let path = write_config(
            dir,
            r#"
[models.default]
model = "openai/gpt-4"
"#,
        )?;

        let config = load_from(&path).map_err(|e| format!("load should succeed: {e:?}"))?;

        assert!(
            config.hooks.is_empty(),
            "a config without [[hooks]] must parse with an empty hooks vec"
        );
        Ok(())
    })
}

// endregion: --- parsing

// region:    --- validation

#[test]
fn load_rejects_an_unknown_hook_event() -> Result<()> {
    with_xdg_config_home(|dir| {
        let path = write_config(
            dir,
            r#"
[[hooks]]
name = "audit"
events = ["turn_started", "bogus_event"]
script = "audit.nu"
"#,
        )?;

        let err = match load_from(&path) {
            Ok(_) => return Err("an unknown hook event must not parse".into()),
            Err(e) => e,
        };

        let TomlConfigError::Hook(message) = err else {
            return Err("expected a Hook error".into());
        };
        assert!(
            message.contains("audit"),
            "the message must name the hook: {message}"
        );
        assert!(
            message.contains("bogus_event"),
            "the message must name the invalid event: {message}"
        );
        Ok(())
    })
}

#[test]
fn load_rejects_a_hook_with_an_empty_name() -> Result<()> {
    with_xdg_config_home(|dir| {
        let path = write_config(
            dir,
            r#"
[[hooks]]
name = ""
events = ["turn_started"]
script = "audit.nu"
"#,
        )?;

        let err = match load_from(&path) {
            Ok(_) => return Err("an empty hook name must not parse".into()),
            Err(e) => e,
        };

        let TomlConfigError::Hook(message) = err else {
            return Err("expected a Hook error".into());
        };
        assert!(
            message.contains("name"),
            "the message must name the offending field: {message}"
        );
        Ok(())
    })
}

#[test]
fn load_rejects_a_hook_with_an_empty_script() -> Result<()> {
    with_xdg_config_home(|dir| {
        let path = write_config(
            dir,
            r#"
[[hooks]]
name = "audit"
events = ["turn_started"]
script = ""
"#,
        )?;

        let err = match load_from(&path) {
            Ok(_) => return Err("an empty hook script must not parse".into()),
            Err(e) => e,
        };

        let TomlConfigError::Hook(message) = err else {
            return Err("expected a Hook error".into());
        };
        assert!(
            message.contains("audit"),
            "the message must name the hook: {message}"
        );
        assert!(
            message.contains("script"),
            "the message must name the offending field: {message}"
        );
        Ok(())
    })
}

// endregion: --- validation

// region:    --- resolution

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

// endregion: --- resolution

// region:    --- event surface

#[test]
fn hook_event_names_and_classes_cover_the_v1_surface() -> Result<()> {
    // -- Setup & Fixtures
    let expected = [
        (
            HookEvent::TurnStarted,
            "turn_started",
            HookEventClass::Session,
        ),
        (
            HookEvent::TurnCompleted,
            "turn_completed",
            HookEventClass::Session,
        ),
        (
            HookEvent::ToolStarted,
            "tool_started",
            HookEventClass::Session,
        ),
        (
            HookEvent::ToolCompleted,
            "tool_completed",
            HookEventClass::Session,
        ),
        (
            HookEvent::SessionStarted,
            "session_started",
            HookEventClass::Session,
        ),
        (
            HookEvent::SessionEnded,
            "session_ended",
            HookEventClass::Session,
        ),
        (
            HookEvent::AgentStarted,
            "agent_started",
            HookEventClass::Agent,
        ),
        (
            HookEvent::AgentShutdown,
            "agent_shutdown",
            HookEventClass::Agent,
        ),
    ];

    // -- Exec & Check
    for (event, name, class) in expected {
        assert_eq!(event.name(), name, "the event name must be {name}");
        assert_eq!(
            event.class(),
            class,
            "{name} must be in the {class:?} class"
        );
        assert_eq!(
            HookEvent::from_name(name),
            Some(event),
            "{name} must parse back to its variant"
        );
    }
    Ok(())
}

#[test]
fn hook_event_serde_round_trips_the_v1_names() -> Result<()> {
    // -- Setup & Fixtures
    let names = [
        "turn_started",
        "turn_completed",
        "tool_started",
        "tool_completed",
        "session_started",
        "session_ended",
        "agent_started",
        "agent_shutdown",
    ];

    // -- Exec & Check
    for name in names {
        let event = HookEvent::from_name(name).ok_or(format!("{name} must be a known event"))?;
        let encoded = serde_json::to_string(&event)?;
        assert_eq!(encoded, format!("\"{name}\""));
        let decoded: HookEvent = serde_json::from_str(&encoded)?;
        assert_eq!(decoded, event);
    }
    Ok(())
}

// endregion: --- event surface
