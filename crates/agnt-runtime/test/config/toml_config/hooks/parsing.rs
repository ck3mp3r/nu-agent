//! `[[hooks]]` parsing: every entry shape the loader accepts.

use super::*;

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
fn load_parses_a_permission_requested_hook() -> Result<()> {
    with_xdg_config_home(|dir| {
        let path = write_config(
            dir,
            r#"
[[hooks]]
name = "audit"
events = ["permission_requested"]
script = "audit.nu"
"#,
        )?;

        let config = load_from(&path).map_err(|e| format!("load should succeed: {e:?}"))?;

        assert_eq!(
            config.hooks[0].events,
            vec![HookEvent::PermissionRequested],
            "permission_requested must be a valid event name"
        );
        Ok(())
    })
}

#[test]
fn load_parses_a_permission_decision_timed_out_hook() -> Result<()> {
    with_xdg_config_home(|dir| {
        let path = write_config(
            dir,
            r#"
[[hooks]]
name = "audit"
events = ["permission_decision_timed_out"]
script = "audit.nu"
"#,
        )?;

        let config = load_from(&path).map_err(|e| format!("load should succeed: {e:?}"))?;

        assert_eq!(
            config.hooks[0].events,
            vec![HookEvent::PermissionDecisionTimedOut],
            "permission_decision_timed_out must be a valid event name"
        );
        Ok(())
    })
}

#[test]
fn load_parses_a_permission_decision_submitted_hook() -> Result<()> {
    with_xdg_config_home(|dir| {
        let path = write_config(
            dir,
            r#"
[[hooks]]
name = "audit"
events = ["permission_decision_submitted"]
script = "audit.nu"
"#,
        )?;

        let config = load_from(&path).map_err(|e| format!("load should succeed: {e:?}"))?;

        assert_eq!(
            config.hooks[0].events,
            vec![HookEvent::PermissionDecisionSubmitted],
            "permission_decision_submitted must be a valid event name"
        );
        Ok(())
    })
}

#[test]
fn load_parses_a_permission_auto_resolved_hook() -> Result<()> {
    with_xdg_config_home(|dir| {
        let path = write_config(
            dir,
            r#"
[[hooks]]
name = "audit"
events = ["permission_auto_resolved"]
script = "audit.nu"
"#,
        )?;

        let config = load_from(&path).map_err(|e| format!("load should succeed: {e:?}"))?;

        assert_eq!(
            config.hooks[0].events,
            vec![HookEvent::PermissionAutoResolved],
            "permission_auto_resolved must be a valid event name"
        );
        Ok(())
    })
}

#[test]
fn load_parses_a_compaction_hook() -> Result<()> {
    with_xdg_config_home(|dir| {
        let path = write_config(
            dir,
            r#"
[[hooks]]
name = "audit"
events = ["compaction_started", "compaction_completed", "compaction_failed"]
script = "audit.nu"
"#,
        )?;

        let config = load_from(&path).map_err(|e| format!("load should succeed: {e:?}"))?;

        assert_eq!(
            config.hooks[0].events,
            vec![
                HookEvent::CompactionStarted,
                HookEvent::CompactionCompleted,
                HookEvent::CompactionFailed
            ],
            "the three compaction names must be valid event names"
        );
        Ok(())
    })
}

#[test]
fn load_parses_an_agent_switched_hook() -> Result<()> {
    with_xdg_config_home(|dir| {
        let path = write_config(
            dir,
            r#"
[[hooks]]
name = "audit"
events = ["agent_switched"]
script = "audit.nu"
"#,
        )?;

        let config = load_from(&path).map_err(|e| format!("load should succeed: {e:?}"))?;

        assert_eq!(
            config.hooks[0].events,
            vec![HookEvent::AgentSwitched],
            "agent_switched must be a valid event name"
        );
        Ok(())
    })
}

#[test]
fn load_parses_a_model_switched_hook() -> Result<()> {
    with_xdg_config_home(|dir| {
        let path = write_config(
            dir,
            r#"
[[hooks]]
name = "audit"
events = ["model_switched"]
script = "audit.nu"
"#,
        )?;

        let config = load_from(&path).map_err(|e| format!("load should succeed: {e:?}"))?;

        assert_eq!(
            config.hooks[0].events,
            vec![HookEvent::ModelSwitched],
            "model_switched must be a valid event name"
        );
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
