use agnt_core::config::Paths;
use agnt_core::utils::env_map::EnvMap;

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
    let err = resolve_persona_from_clap(
        None,
        None,
        &agents_config,
        std::path::Path::new("."),
        std::path::Path::new("."),
    )
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
        resolve_persona_from_clap(Some("plain".to_string()), None, &agents_config, &dir, &dir)?;

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
        &dir,
    )
    .err()
    .ok_or("should reject missing persona")?;

    // -- Check
    assert!(err.to_string().contains("not found"), "err={err}");

    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

/// A persona in the resolved `Paths.agents_dir` resolves through the threaded
/// site: the resolver takes the directory verbatim and never joins a second
/// `agents` segment onto it.
#[test]
fn test_dispatch_persona_resolves_from_resolved_paths_agents_dir() -> Result<()> {
    // -- Setup & Fixtures
    let config_home = unique_temp_dir("paths-config")?;
    let xdg_root = unique_temp_dir("paths-xdg")?;
    let env = EnvMap::from([
        (
            "NU_AGENT_CONFIG_HOME".to_string(),
            config_home.to_string_lossy().to_string(),
        ),
        (
            "XDG_DATA_HOME".to_string(),
            xdg_root.join("data").to_string_lossy().to_string(),
        ),
        (
            "XDG_CACHE_HOME".to_string(),
            xdg_root.join("cache").to_string_lossy().to_string(),
        ),
        (
            "XDG_STATE_HOME".to_string(),
            xdg_root.join("state").to_string_lossy().to_string(),
        ),
    ]);
    let paths = Paths::resolve_for_process_with(None, &env)?;
    std::fs::create_dir_all(&paths.agents_dir)?;
    std::fs::write(
        paths.agents_dir.join("paths-agent.md"),
        "---\nname: Paths Agent\ndescription: from the resolved dir\n---\nBody text",
    )?;
    let cwd = unique_temp_dir("paths-cwd")?;
    let agents_config = AgentsConfig::default();

    // -- Exec
    let resolution = resolve_persona_from_clap(
        Some("paths-agent".to_string()),
        None,
        &agents_config,
        &cwd,
        &paths.agents_dir,
    )?;

    // -- Check
    let persona = resolution
        .persona
        .ok_or("a persona in Paths.agents_dir should resolve")?;
    assert_eq!(persona.name.as_deref(), Some("Paths Agent"));
    assert_eq!(
        persona.description.as_deref(),
        Some("from the resolved dir")
    );
    assert_eq!(persona.body.trim(), "Body text");

    std::fs::remove_dir_all(&config_home)?;
    std::fs::remove_dir_all(&xdg_root)?;
    std::fs::remove_dir_all(&cwd)?;
    Ok(())
}
