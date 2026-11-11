use super::*;

#[test]
fn list_available_scenarios() -> Result<()> {
    // No agent directories: the listing is empty.
    let temp_cwd = TempDir::new()?;
    let temp_config = TempDir::new()?;
    let resolver = FsPersonaResolver::new(
        temp_cwd.path().to_path_buf(),
        temp_config.path().join("agents"),
        no_builtins(),
    );
    let result = resolver.list_available();
    assert!(result.is_empty());

    // cwd .agents entries are discovered.
    let temp_cwd = TempDir::new()?;
    let temp_config = TempDir::new()?;

    let agents_dir = temp_cwd.path().join(".agents");
    fs::create_dir_all(&agents_dir)?;
    fs::write(
        agents_dir.join("coder.md"),
        "---\nname: coder\ndescription: Writes code\n---\n# Coder",
    )?;

    let resolver = FsPersonaResolver::new(
        temp_cwd.path().to_path_buf(),
        temp_config.path().join("agents"),
        no_builtins(),
    );
    let result = resolver.list_available();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].name, "coder");
    assert_eq!(result[0].description.as_deref(), Some("Writes code"));
    assert!(!result[0].builtin);

    // The filename stem is used when front matter has no name.
    let temp_cwd = TempDir::new()?;
    let temp_config = TempDir::new()?;

    let agents_dir = temp_cwd.path().join(".agents");
    fs::create_dir_all(&agents_dir)?;
    fs::write(
        agents_dir.join("my-agent.md"),
        "# Just a body, no front matter",
    )?;

    let resolver = FsPersonaResolver::new(
        temp_cwd.path().to_path_buf(),
        temp_config.path().join("agents"),
        no_builtins(),
    );
    let result = resolver.list_available();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].name, "my-agent");
    assert_eq!(result[0].description, None);

    // Non-markdown files are skipped.
    let temp_cwd = TempDir::new()?;
    let temp_config = TempDir::new()?;

    let agents_dir = temp_cwd.path().join(".agents");
    fs::create_dir_all(&agents_dir)?;
    fs::write(agents_dir.join("coder.md"), "---\nname: coder\n---\n")?;
    fs::write(agents_dir.join("notes.txt"), "not a persona")?;
    fs::write(agents_dir.join("config.yaml"), "key: value")?;

    let resolver = FsPersonaResolver::new(
        temp_cwd.path().to_path_buf(),
        temp_config.path().join("agents"),
        no_builtins(),
    );
    let result = resolver.list_available();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].name, "coder");

    // cwd and config agents are merged.
    let temp_cwd = TempDir::new()?;
    let temp_config = TempDir::new()?;

    let cwd_agents = temp_cwd.path().join(".agents");
    fs::create_dir_all(&cwd_agents)?;
    fs::write(cwd_agents.join("coder.md"), "---\nname: coder\n---\n")?;

    let xdg_agents = temp_config.path().join("agents");
    fs::create_dir_all(&xdg_agents)?;
    fs::write(xdg_agents.join("reviewer.md"), "---\nname: reviewer\n---\n")?;

    let resolver = FsPersonaResolver::new(
        temp_cwd.path().to_path_buf(),
        temp_config.path().join("agents"),
        no_builtins(),
    );
    let result = resolver.list_available();
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].name, "coder");
    assert_eq!(result[1].name, "reviewer");

    // cwd entries win over config entries with the same filename.
    let temp_cwd = TempDir::new()?;
    let temp_config = TempDir::new()?;

    let cwd_agents = temp_cwd.path().join(".agents");
    fs::create_dir_all(&cwd_agents)?;
    fs::write(
        cwd_agents.join("coder.md"),
        "---\nname: local-coder\ndescription: Local\n---\n",
    )?;

    let xdg_agents = temp_config.path().join("agents");
    fs::create_dir_all(&xdg_agents)?;
    fs::write(
        xdg_agents.join("coder.md"),
        "---\nname: global-coder\ndescription: Global\n---\n",
    )?;

    let resolver = FsPersonaResolver::new(
        temp_cwd.path().to_path_buf(),
        temp_config.path().join("agents"),
        no_builtins(),
    );
    let result = resolver.list_available();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].name, "local-coder");
    assert_eq!(result[0].description.as_deref(), Some("Local"));
    Ok(())
}
