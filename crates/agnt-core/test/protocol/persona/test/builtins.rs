use super::*;

#[test]
fn resolve_and_list_builtin_personas() -> Result<()> {
    // An enabled builtin resolves to its embedded content.
    let temp_cwd = TempDir::new().unwrap();
    let temp_config = TempDir::new().unwrap();

    let resolver = FsPersonaResolver::new(
        temp_cwd.path().to_path_buf(),
        temp_config.path().join("agents"),
        AgentsConfig::default(),
    );
    let (path, content) = resolver
        .resolve("planner")
        .map_err(|e| format!("resolve should succeed: {e:?}"))?;
    assert!(
        path.to_str()
            .ok_or("path should be valid UTF-8")?
            .contains("<builtin>")
    );
    assert_eq!(content, BUILTIN_PLANNER_CONTENT);

    // A disabled builtin falls through to the filesystem.
    let temp_cwd = TempDir::new().unwrap();
    let temp_config = TempDir::new().unwrap();

    let agents_dir = temp_cwd.path().join(".agents");
    fs::create_dir_all(&agents_dir).unwrap();
    let fs_path = agents_dir.join("planner.md");
    fs::write(&fs_path, "# Custom planner").unwrap();

    let config = AgentsConfig {
        planner_enabled: false,
        ..AgentsConfig::default()
    };
    let resolver = FsPersonaResolver::new(
        temp_cwd.path().to_path_buf(),
        temp_config.path().join("agents"),
        config,
    );
    let (path, content) = resolver
        .resolve("planner")
        .map_err(|e| format!("resolve should succeed: {e:?}"))?;
    assert_eq!(path, fs_path);
    assert_eq!(content, "# Custom planner");

    // A plain filesystem persona still resolves.
    let temp_cwd = TempDir::new().unwrap();
    let temp_config = TempDir::new().unwrap();

    let agents_dir = temp_cwd.path().join(".agents");
    fs::create_dir_all(&agents_dir).unwrap();
    let persona_file = agents_dir.join("custom.md");
    fs::write(&persona_file, "# Custom persona").unwrap();

    let resolver = FsPersonaResolver::new(
        temp_cwd.path().to_path_buf(),
        temp_config.path().join("agents"),
        AgentsConfig::default(),
    );
    let (path, content) = resolver
        .resolve("custom")
        .map_err(|e| format!("resolve should succeed: {e:?}"))?;
    assert_eq!(path, persona_file);
    assert_eq!(content, "# Custom persona");

    // Enabled builtins are the first listing entries.
    let temp_cwd = TempDir::new().unwrap();
    let temp_config = TempDir::new().unwrap();

    let resolver = FsPersonaResolver::new(
        temp_cwd.path().to_path_buf(),
        temp_config.path().join("agents"),
        AgentsConfig::default(),
    );
    let result = resolver.list_available();

    assert!(result.len() >= 2);
    assert_eq!(result[0].name, "planner");
    assert!(result[0].builtin);
    assert_eq!(result[1].name, "maker");
    assert!(result[1].builtin);

    // A disabled builtin is excluded from the listing.
    let temp_cwd = TempDir::new().unwrap();
    let temp_config = TempDir::new().unwrap();

    let config = AgentsConfig {
        planner_enabled: false,
        ..AgentsConfig::default()
    };
    let resolver = FsPersonaResolver::new(
        temp_cwd.path().to_path_buf(),
        temp_config.path().join("agents"),
        config,
    );
    let result = resolver.list_available();

    let planner_builtins: Vec<_> = result
        .iter()
        .filter(|s| s.name == "planner" && s.builtin)
        .collect();
    assert!(planner_builtins.is_empty());
    assert!(result.iter().any(|s| s.name == "maker" && s.builtin));

    // A filesystem persona colliding with a builtin is deduplicated.
    let temp_cwd = TempDir::new().unwrap();
    let temp_config = TempDir::new().unwrap();

    let agents_dir = temp_cwd.path().join(".agents");
    fs::create_dir_all(&agents_dir).unwrap();
    fs::write(
        agents_dir.join("planner.md"),
        "---\nname: planner\ndescription: Custom planner\n---\n",
    )
    .unwrap();

    let resolver = FsPersonaResolver::new(
        temp_cwd.path().to_path_buf(),
        temp_config.path().join("agents"),
        AgentsConfig::default(),
    );
    let result = resolver.list_available();

    let planner_entries: Vec<_> = result.iter().filter(|s| s.name == "planner").collect();
    assert_eq!(planner_entries.len(), 1);
    assert!(planner_entries[0].builtin);
    Ok(())
}

#[test]
fn resolve_builtin_planner_disabled_not_found() {
    let temp_cwd = TempDir::new().unwrap();
    let temp_config = TempDir::new().unwrap();

    let config = AgentsConfig {
        planner_enabled: false,
        ..AgentsConfig::default()
    };
    let resolver = FsPersonaResolver::new(
        temp_cwd.path().to_path_buf(),
        temp_config.path().join("agents"),
        config,
    );
    let result = resolver.resolve("planner");

    assert!(result.is_err());
    assert!(matches!(result, Err(PersonaError::NotFound { .. })));
}
