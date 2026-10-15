use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Test Support

fn unique_temp_dir(tag: &str) -> Result<std::path::PathBuf> {
    let dir = std::env::temp_dir().join(format!("agnt-cli-persona-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

// endregion: --- Test Support

#[test]
fn test_dispatch_persona_disabled_builtin_errors() -> Result<()> {
    // -- Setup & Fixtures
    let agents_config = AgentsConfig {
        planner_enabled: false,
        default: "planner".to_string(),
        ..AgentsConfig::default()
    };

    // -- Exec
    let err = resolve_persona_from_clap(
        Some("planner".to_string()),
        None,
        &agents_config,
        std::path::Path::new("."),
    )
    .err()
    .ok_or("should reject disabled builtin persona")?;

    // -- Check
    assert!(err.to_string().contains("disabled"), "err={err}");
    Ok(())
}

#[test]
fn test_dispatch_persona_default_disabled_without_fallback_errors() -> Result<()> {
    // -- Setup & Fixtures
    let agents_config = AgentsConfig {
        planner_enabled: false,
        default: "planner".to_string(),
        fallback: None,
        ..AgentsConfig::default()
    };

    // -- Exec
    let err = resolve_persona_from_clap(None, None, &agents_config, std::path::Path::new("."))
        .err()
        .ok_or("should reject disabled default without fallback")?;

    // -- Check
    assert!(err.to_string().contains("disabled"), "err={err}");
    Ok(())
}

#[test]
fn test_dispatch_persona_resolves_from_cwd_file() -> Result<()> {
    // -- Setup & Fixtures
    let dir = unique_temp_dir("cwd")?;
    let agents_dir = dir.join(".agents");
    std::fs::create_dir_all(&agents_dir)?;
    std::fs::write(
        agents_dir.join("my-agent.md"),
        "---\nname: My Agent\ndescription: does things\n---\nBody text",
    )?;
    let agents_config = AgentsConfig::default();

    // -- Exec
    let resolution = resolve_persona_from_clap(
        Some("my-agent".to_string()),
        Some("cli-name".to_string()),
        &agents_config,
        &dir,
    )?;

    // -- Check
    let persona = resolution.persona.ok_or("should resolve persona")?;
    assert_eq!(persona.name.as_deref(), Some("My Agent"));
    assert_eq!(persona.description.as_deref(), Some("does things"));
    assert_eq!(persona.body.trim(), "Body text");
    assert_eq!(resolution.agent_identity.as_deref(), Some("My Agent"));
    assert_eq!(resolution.messaging_identity.as_deref(), Some("cli-name"));

    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn test_dispatch_persona_messaging_identity_falls_back_to_agent_identity() -> Result<()> {
    // -- Setup & Fixtures
    let dir = unique_temp_dir("identity")?;
    let agents_dir = dir.join(".agents");
    std::fs::create_dir_all(&agents_dir)?;
    std::fs::write(agents_dir.join("plain.md"), "# Plain\nBody")?;
    let agents_config = AgentsConfig::default();

    // -- Exec
    let resolution =
        resolve_persona_from_clap(Some("plain".to_string()), None, &agents_config, &dir)?;

    // -- Check
    assert_eq!(resolution.agent_identity.as_deref(), Some("plain"));
    assert_eq!(resolution.messaging_identity.as_deref(), Some("plain"));
    assert!(resolution.agent_permissions_overlay.is_none());

    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn test_dispatch_persona_missing_persona_errors() -> Result<()> {
    // -- Setup & Fixtures
    let dir = unique_temp_dir("missing")?;
    let agents_config = AgentsConfig::default();

    // -- Exec
    let err = resolve_persona_from_clap(
        Some("does-not-exist".to_string()),
        None,
        &agents_config,
        &dir,
    )
    .err()
    .ok_or("should reject missing persona")?;

    // -- Check
    assert!(err.to_string().contains("not found"), "err={err}");

    std::fs::remove_dir_all(&dir)?;
    Ok(())
}
