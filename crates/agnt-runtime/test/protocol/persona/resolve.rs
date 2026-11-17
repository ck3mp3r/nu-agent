use crate::config::AgentsConfig;
use crate::protocol::persona::resolve::resolve_persona;
use nu_protocol::Span;
use tempfile::TempDir;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Test Support

fn write_cwd_persona(dir: &std::path::Path, name: &str, contents: &str) -> Result<()> {
    let agents_dir = dir.join(".agents");
    std::fs::create_dir_all(&agents_dir)?;
    std::fs::write(agents_dir.join(format!("{name}.md")), contents)?;
    Ok(())
}

// endregion: --- Test Support

#[test]
fn test_protocol_persona_resolve_disabled_builtin_flag_errors() -> Result<()> {
    // -- Setup & Fixtures
    let agents_config = AgentsConfig {
        planner_enabled: false,
        default: "planner".to_string(),
        ..AgentsConfig::default()
    };
    let cwd = TempDir::new()?;
    let span = Span::unknown();

    // -- Exec
    let err = resolve_persona(
        Some("planner".to_string()),
        None,
        &agents_config,
        cwd.path(),
        cwd.path(),
        span,
    )
    .err()
    .ok_or("should reject a disabled builtin persona")?;

    // -- Check
    assert_eq!(
        err.msg,
        "Agent 'planner' is disabled in config. Enable it or use a different agent."
    );
    assert!(
        err.labels
            .iter()
            .any(|label| label.text == "disabled agent" && label.span == span),
        "should label the flagged disabled agent: {:?}",
        err.labels
    );
    Ok(())
}

#[test]
fn test_protocol_persona_resolve_disabled_default_without_fallback_errors() -> Result<()> {
    // -- Setup & Fixtures
    let agents_config = AgentsConfig {
        planner_enabled: false,
        default: "planner".to_string(),
        fallback: None,
        ..AgentsConfig::default()
    };
    let cwd = TempDir::new()?;

    // -- Exec
    let err = resolve_persona(
        None,
        None,
        &agents_config,
        cwd.path(),
        cwd.path(),
        Span::unknown(),
    )
    .err()
    .ok_or("should reject a disabled default without fallback")?;

    // -- Check
    assert!(err.msg.contains("disabled"), "msg={}", err.msg);
    Ok(())
}

#[test]
fn test_protocol_persona_resolve_missing_persona_errors() -> Result<()> {
    // -- Setup & Fixtures
    let agents_config = AgentsConfig::default();
    let cwd = TempDir::new()?;
    let span = Span::unknown();

    // -- Exec
    let err = resolve_persona(
        Some("does-not-exist".to_string()),
        None,
        &agents_config,
        cwd.path(),
        cwd.path(),
        span,
    )
    .err()
    .ok_or("should reject a missing persona")?;

    // -- Check
    assert!(
        err.msg.starts_with("Agent persona not found: "),
        "msg={}",
        err.msg
    );
    assert!(
        err.labels
            .iter()
            .any(|label| label.span == span && label.text.contains("does-not-exist")),
        "should label the lookup detail: {:?}",
        err.labels
    );
    Ok(())
}

#[test]
fn test_protocol_persona_resolve_full_persona_from_cwd() -> Result<()> {
    // -- Setup & Fixtures
    let dir = TempDir::new()?;
    write_cwd_persona(
        dir.path(),
        "my-agent",
        "---\nname: My Agent\ndescription: does things\n---\nBody text",
    )?;
    let agents_config = AgentsConfig::default();

    // -- Exec
    let resolution = resolve_persona(
        Some("my-agent".to_string()),
        Some("cli-name".to_string()),
        &agents_config,
        dir.path(),
        dir.path(),
        Span::unknown(),
    )?;

    // -- Check
    let persona = resolution.persona.ok_or("should resolve the persona")?;
    assert_eq!(persona.name.as_deref(), Some("My Agent"));
    assert_eq!(persona.description.as_deref(), Some("does things"));
    assert_eq!(persona.body.trim(), "Body text");
    assert_eq!(resolution.agent_identity.as_deref(), Some("My Agent"));
    assert_eq!(resolution.agent_slug.as_deref(), Some("my-agent"));
    assert_eq!(resolution.messaging_identity.as_deref(), Some("cli-name"));
    Ok(())
}

#[test]
fn test_protocol_persona_resolve_messaging_identity_falls_back_and_overlay_absent() -> Result<()> {
    // -- Setup & Fixtures
    let dir = TempDir::new()?;
    write_cwd_persona(dir.path(), "plain", "# Plain\nBody")?;
    let agents_config = AgentsConfig::default();

    // -- Exec
    let resolution = resolve_persona(
        Some("plain".to_string()),
        None,
        &agents_config,
        dir.path(),
        dir.path(),
        Span::unknown(),
    )?;

    // -- Check
    assert_eq!(resolution.agent_identity.as_deref(), Some("plain"));
    assert_eq!(resolution.messaging_identity.as_deref(), Some("plain"));
    assert!(resolution.agent_permissions_overlay.is_none());
    Ok(())
}

#[test]
fn test_protocol_persona_resolve_invalid_front_matter_errors() -> Result<()> {
    // -- Setup & Fixtures
    let dir = TempDir::new()?;
    write_cwd_persona(dir.path(), "bad-yaml", "---\n[invalid: yaml:\n---\n# Body")?;
    let agents_config = AgentsConfig::default();
    let span = Span::unknown();

    // -- Exec
    let err = resolve_persona(
        Some("bad-yaml".to_string()),
        None,
        &agents_config,
        dir.path(),
        dir.path(),
        span,
    )
    .err()
    .ok_or("should reject invalid front matter")?;

    // -- Check
    assert!(
        err.msg.starts_with("Invalid agent persona front matter: "),
        "msg={}",
        err.msg
    );
    assert!(
        err.labels.iter().any(|label| label.span == span),
        "should label the front-matter detail: {:?}",
        err.labels
    );
    Ok(())
}

#[test]
fn test_protocol_persona_resolve_invalid_permissions_errors() -> Result<()> {
    // -- Setup & Fixtures
    let dir = TempDir::new()?;
    write_cwd_persona(
        dir.path(),
        "bad-perms",
        "---\nname: Bad Perms\npermissions:\n  \"*\": nonsense\n---\nBody",
    )?;
    let agents_config = AgentsConfig::default();
    let span = Span::unknown();

    // -- Exec
    let err = resolve_persona(
        Some("bad-perms".to_string()),
        None,
        &agents_config,
        dir.path(),
        dir.path(),
        span,
    )
    .err()
    .ok_or("should reject invalid persona permissions")?;

    // -- Check
    assert!(
        err.msg.starts_with("Invalid agent permissions: "),
        "msg={}",
        err.msg
    );
    assert!(
        err.labels
            .iter()
            .any(|label| label.span == span && label.text.contains("invalid permission action")),
        "should label the permissions detail: {:?}",
        err.labels
    );
    Ok(())
}
