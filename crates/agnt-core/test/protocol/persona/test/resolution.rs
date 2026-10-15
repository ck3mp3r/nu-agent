use super::*;

#[test]
fn resolve_persona_from_sources() -> Result<()> {
    // A cwd .agents persona resolves.
    let temp_cwd = TempDir::new().unwrap();
    let temp_config = TempDir::new().unwrap();

    let agents_dir = temp_cwd.path().join(".agents");
    fs::create_dir_all(&agents_dir).unwrap();
    let persona_file = agents_dir.join("test-agent.md");
    fs::write(&persona_file, "# Test Agent\nContent here").unwrap();

    let resolver = FsPersonaResolver::new(
        temp_cwd.path().to_path_buf(),
        temp_config.path().to_path_buf(),
        AgentsConfig::default(),
    );
    let (path, content) = resolver
        .resolve("test-agent")
        .map_err(|e| format!("resolve should succeed: {e:?}"))?;
    assert_eq!(path, persona_file);
    assert_eq!(content, "# Test Agent\nContent here");

    // A config agents persona resolves.
    let temp_cwd = TempDir::new().unwrap();
    let temp_config = TempDir::new().unwrap();

    let agents_dir = temp_config.path().join("agents");
    fs::create_dir_all(&agents_dir).unwrap();
    let persona_file = agents_dir.join("test-agent.md");
    fs::write(&persona_file, "# Home Agent\nHome content").unwrap();

    let resolver = FsPersonaResolver::new(
        temp_cwd.path().to_path_buf(),
        temp_config.path().to_path_buf(),
        AgentsConfig::default(),
    );
    let (path, content) = resolver
        .resolve("test-agent")
        .map_err(|e| format!("resolve should succeed: {e:?}"))?;
    assert_eq!(path, persona_file);
    assert_eq!(content, "# Home Agent\nHome content");

    // cwd takes precedence over config.
    let temp_cwd = TempDir::new().unwrap();
    let temp_config = TempDir::new().unwrap();

    let cwd_agents_dir = temp_cwd.path().join(".agents");
    fs::create_dir_all(&cwd_agents_dir).unwrap();
    let cwd_persona = cwd_agents_dir.join("test-agent.md");
    fs::write(&cwd_persona, "# CWD Agent").unwrap();

    let config_agents_dir = temp_config.path().join("agents");
    fs::create_dir_all(&config_agents_dir).unwrap();
    let config_persona = config_agents_dir.join("test-agent.md");
    fs::write(&config_persona, "# Config Agent").unwrap();

    let resolver = FsPersonaResolver::new(
        temp_cwd.path().to_path_buf(),
        temp_config.path().to_path_buf(),
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
fn resolve_not_found_lists_both_paths() {
    let temp_cwd = TempDir::new().unwrap();
    let temp_config = TempDir::new().unwrap();

    let resolver = FsPersonaResolver::new(
        temp_cwd.path().to_path_buf(),
        temp_config.path().to_path_buf(),
        AgentsConfig::default(),
    );
    let result = resolver.resolve("nonexistent");

    assert!(result.is_err());
    if let Err(PersonaError::NotFound {
        cwd_path,
        config_path,
    }) = result
    {
        assert_eq!(
            cwd_path,
            temp_cwd.path().join(".agents").join("nonexistent.md")
        );
        assert_eq!(
            config_path,
            temp_config.path().join("agents").join("nonexistent.md")
        );
    } else {
        panic!("Expected NotFound error");
    }
}

#[test]
fn resolve_read_error() {
    let temp_cwd = TempDir::new().unwrap();
    let temp_config = TempDir::new().unwrap();

    let agents_dir = temp_cwd.path().join(".agents");
    fs::create_dir_all(&agents_dir).unwrap();
    let persona_file = agents_dir.join("test-agent.md");
    fs::write(&persona_file, "content").unwrap();

    // Make file unreadable (Unix permissions)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&persona_file).unwrap().permissions();
        perms.set_mode(0o000);
        fs::set_permissions(&persona_file, perms).unwrap();
    }

    let resolver = FsPersonaResolver::new(
        temp_cwd.path().to_path_buf(),
        temp_config.path().to_path_buf(),
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
        let mut perms = fs::metadata(&persona_file).unwrap().permissions();
        perms.set_mode(0o644);
        let _ = fs::set_permissions(&persona_file, perms);
    }
}
