use super::*;

#[test]
fn resolve_persona_from_sources() -> Result<()> {
    // A cwd .agents persona resolves.
    let temp_cwd = TempDir::new()?;
    let temp_config = TempDir::new()?;

    let agents_dir = temp_cwd.path().join(".agents");
    fs::create_dir_all(&agents_dir)?;
    let persona_file = agents_dir.join("test-agent.md");
    fs::write(&persona_file, "# Test Agent\nContent here")?;

    let resolver = FsPersonaResolver::new(
        temp_cwd.path().to_path_buf(),
        temp_config.path().join("agents"),
        AgentsConfig::default(),
    );
    let (path, content) = resolver
        .resolve("test-agent")
        .map_err(|e| format!("resolve should succeed: {e:?}"))?;
    assert_eq!(path, persona_file);
    assert_eq!(content, "# Test Agent\nContent here");

    // A config agents persona resolves.
    let temp_cwd = TempDir::new()?;
    let temp_config = TempDir::new()?;

    let agents_dir = temp_config.path().join("agents");
    fs::create_dir_all(&agents_dir)?;
    let persona_file = agents_dir.join("test-agent.md");
    fs::write(&persona_file, "# Home Agent\nHome content")?;

    let resolver = FsPersonaResolver::new(
        temp_cwd.path().to_path_buf(),
        temp_config.path().join("agents"),
        AgentsConfig::default(),
    );
    let (path, content) = resolver
        .resolve("test-agent")
        .map_err(|e| format!("resolve should succeed: {e:?}"))?;
    assert_eq!(path, persona_file);
    assert_eq!(content, "# Home Agent\nHome content");

    // cwd takes precedence over config.
    let temp_cwd = TempDir::new()?;
    let temp_config = TempDir::new()?;

    let cwd_agents_dir = temp_cwd.path().join(".agents");
    fs::create_dir_all(&cwd_agents_dir)?;
    let cwd_persona = cwd_agents_dir.join("test-agent.md");
    fs::write(&cwd_persona, "# CWD Agent")?;

    let config_agents_dir = temp_config.path().join("agents");
    fs::create_dir_all(&config_agents_dir)?;
    let config_persona = config_agents_dir.join("test-agent.md");
    fs::write(&config_persona, "# Config Agent")?;

    let resolver = FsPersonaResolver::new(
        temp_cwd.path().to_path_buf(),
        temp_config.path().join("agents"),
        AgentsConfig::default(),
    );
    let (path, content) = resolver
        .resolve("test-agent")
        .map_err(|e| format!("resolve should succeed: {e:?}"))?;
    assert_eq!(path, cwd_persona);
    assert_eq!(content, "# CWD Agent");
    Ok(())
}

#[test]
fn resolve_not_found_lists_both_paths() -> Result<()> {
    let temp_cwd = TempDir::new()?;
    let temp_config = TempDir::new()?;

    let resolver = FsPersonaResolver::new(
        temp_cwd.path().to_path_buf(),
        temp_config.path().join("agents"),
        AgentsConfig::default(),
    );
    let result = resolver.resolve("nonexistent");

    assert!(result.is_err());
    if let Err(PersonaError::NotFound {
        cwd_path,
        agents_path,
    }) = result
    {
        assert_eq!(
            cwd_path,
            temp_cwd.path().join(".agents").join("nonexistent.md")
        );
        assert_eq!(
            agents_path,
            temp_config.path().join("agents").join("nonexistent.md")
        );
    } else {
        panic!("Expected NotFound error");
    }

    Ok(())
}

#[test]
fn resolve_read_error() -> Result<()> {
    let temp_cwd = TempDir::new()?;
    let temp_config = TempDir::new()?;

    let agents_dir = temp_cwd.path().join(".agents");
    fs::create_dir_all(&agents_dir)?;
    let persona_file = agents_dir.join("test-agent.md");
    fs::write(&persona_file, "content")?;

    // Make file unreadable (Unix permissions)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&persona_file)?.permissions();
        perms.set_mode(0o000);
        fs::set_permissions(&persona_file, perms)?;
    }

    let resolver = FsPersonaResolver::new(
        temp_cwd.path().to_path_buf(),
        temp_config.path().join("agents"),
        AgentsConfig::default(),
    );
    let result = resolver.resolve("test-agent");

    #[cfg(unix)]
    {
        assert!(result.is_err());
        if let Err(PersonaError::ReadFailed { path, .. }) = result {
            assert_eq!(path, persona_file);
        } else {
            panic!("Expected ReadFailed error");
        }
    }

    // Cleanup: restore permissions for temp dir cleanup
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&persona_file)?.permissions();
        perms.set_mode(0o644);
        let _ = fs::set_permissions(&persona_file, perms);
    }

    Ok(())
}

#[test]
fn resolve_rejects_path_separators() -> Result<()> {
    // -- Setup & Fixtures
    let temp_cwd = TempDir::new().map_err(|e| format!("temp cwd: {e}"))?;
    let temp_config = TempDir::new().map_err(|e| format!("temp config: {e}"))?;
    let resolver = FsPersonaResolver::new(
        temp_cwd.path().to_path_buf(),
        temp_config.path().join("agents"),
        AgentsConfig::default(),
    );

    // -- Exec & Check
    for name in ["../../x", "a/b", "a\\b", "/etc/passwd", "..\\..\\x"] {
        let err = resolver
            .resolve(name)
            .err()
            .ok_or("resolve should reject separator names")?;
        assert!(
            matches!(err, PersonaError::InvalidName { .. }),
            "expected InvalidName for {name:?}, got {err:?}"
        );
    }
    Ok(())
}

#[test]
fn resolve_rejects_dot_and_empty_names() -> Result<()> {
    // -- Setup & Fixtures
    let temp_cwd = TempDir::new().map_err(|e| format!("temp cwd: {e}"))?;
    let temp_config = TempDir::new().map_err(|e| format!("temp config: {e}"))?;
    let resolver = FsPersonaResolver::new(
        temp_cwd.path().to_path_buf(),
        temp_config.path().join("agents"),
        AgentsConfig::default(),
    );

    // -- Exec & Check
    for name in ["", ".", ".."] {
        let err = resolver
            .resolve(name)
            .err()
            .ok_or("resolve should reject dot and empty names")?;
        assert!(
            matches!(err, PersonaError::InvalidName { .. }),
            "expected InvalidName for {name:?}, got {err:?}"
        );
    }
    Ok(())
}

#[test]
fn resolve_rejects_traversal_without_reading_outside_file() -> Result<()> {
    // -- Setup & Fixtures
    let temp_cwd = TempDir::new().map_err(|e| format!("temp cwd: {e}"))?;
    let temp_config = TempDir::new().map_err(|e| format!("temp config: {e}"))?;

    // A file that `../secret` would reach if traversal were allowed.
    let outside_file = temp_cwd.path().join("secret.md");
    fs::write(&outside_file, "# Escaped persona").map_err(|e| format!("write secret: {e}"))?;

    let resolver = FsPersonaResolver::new(
        temp_cwd.path().to_path_buf(),
        temp_config.path().join("agents"),
        AgentsConfig::default(),
    );

    // -- Exec
    let err = resolver
        .resolve("../secret")
        .err()
        .ok_or("resolve should reject traversal name")?;

    // -- Check
    assert!(
        matches!(err, PersonaError::InvalidName { .. }),
        "expected InvalidName, got {err:?}"
    );
    assert!(err.to_string().contains("../secret"));
    Ok(())
}
