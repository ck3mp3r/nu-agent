use super::*;

// === Tests: agents ===

#[test]
fn loads_agents_from_sources() -> Result<()> {
    // Home config agents are loaded.
    let tmp = tempdir()?;
    let config = tmp.path().join("config");
    let cwd = tmp.path().join("cwd");
    fs::create_dir_all(config.join("agents"))?;
    fs::create_dir_all(&cwd)?;
    fs::write(config.join("agents/AGENTS.md"), "CONFIG\n")?;

    let loaded = load_agents_chain(&cwd, Some(&config.join("agents")), Some(tmp.path()), None);

    assert_eq!(loaded.merged_chain.as_deref(), Some("CONFIG\n"));
    assert!(loaded.warnings.is_empty());

    // cwd agents are loaded.
    let tmp = tempdir()?;
    let cwd = tmp.path().join("cwd");
    fs::create_dir_all(&cwd)?;
    fs::write(cwd.join("AGENTS.md"), "CWD\n")?;

    let loaded = load_agents_chain(&cwd, None, Some(tmp.path()), None);

    assert_eq!(loaded.merged_chain.as_deref(), Some("CWD\n"));
    assert!(loaded.warnings.is_empty());

    // Ancestor agents are merged root to leaf.
    let tmp = tempdir()?;
    let root = tmp.path().join("root");
    let parent = root.join("a");
    let cwd = parent.join("b");
    fs::create_dir_all(&cwd)?;
    fs::write(root.join("AGENTS.md"), "ROOT\n")?;
    fs::write(parent.join("AGENTS.md"), "PARENT\n")?;

    let loaded = load_agents_chain(&cwd, None, Some(&root), None);

    assert_eq!(loaded.merged_chain.as_deref(), Some("ROOT\n\nPARENT\n"));
    assert!(loaded.warnings.is_empty());

    // Missing home or agents files are a no-op.
    let tmp = tempdir()?;
    let cwd = tmp.path().join("cwd");
    fs::create_dir_all(&cwd)?;

    let loaded = load_agents_chain(&cwd, None, Some(tmp.path()), None);

    assert_eq!(loaded.merged_chain, None);
    assert!(loaded.warnings.is_empty());

    // An unreadable AGENTS.md is non-fatal.
    let tmp = tempdir()?;
    let cwd = tmp.path().join("cwd");
    let agents = cwd.join("AGENTS.md");
    fs::create_dir_all(&agents)?;

    let loaded = load_agents_chain(&cwd, None, Some(tmp.path()), None);

    assert!(loaded.warnings.iter().any(|w| w.contains("AGENTS.md")));
    assert_eq!(loaded.merged_chain, None);

    Ok(())
}

#[test]
fn agents_precedence_and_dedup() -> Result<()> {
    // The nearest AGENTS.md has the highest precedence position.
    let tmp = tempdir()?;
    let root = tmp.path().join("root");
    let cwd = root.join("child");
    fs::create_dir_all(&cwd)?;
    fs::write(root.join("AGENTS.md"), "ROOT\n")?;
    fs::write(cwd.join("AGENTS.md"), "CWD\n")?;

    let loaded = load_agents_chain(&cwd, None, Some(&root), None);
    let merged = loaded
        .merged_chain
        .ok_or("merged chain should be present")?;

    assert!(merged.ends_with("CWD\n"));
    assert!(merged.contains("ROOT\n"));

    // Canonical path dedup prevents a duplicate load through a symlink.
    let tmp = tempdir()?;
    let root = tmp.path().join("root");
    let real = root.join("real");
    let alias = root.join("alias");
    fs::create_dir_all(&real)?;
    fs::write(real.join("AGENTS.md"), "REAL\n")?;

    #[cfg(unix)]
    std::os::unix::fs::symlink(&real, &alias)?;

    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&real, &alias)?;

    let cwd = alias.join("child");
    fs::create_dir_all(&cwd)?;

    let loaded = load_agents_chain(&cwd, None, Some(&root), None);
    let merged = loaded
        .merged_chain
        .ok_or("merged chain should be present")?;

    assert_eq!(merged.matches("REAL").count(), 1);

    // Home ~/.agents/AGENTS.md is loaded.
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let cwd = tmp.path().join("cwd");
    fs::create_dir_all(home.join(".agents"))?;
    fs::create_dir_all(&cwd)?;
    fs::write(home.join(".agents/AGENTS.md"), "HOME_AGENTS\n")?;

    let result = load_agents_chain(&cwd, None, Some(tmp.path()), Some(&home));
    let merged = result
        .merged_chain
        .ok_or("merged chain should be present")?;
    assert!(merged.contains("HOME_AGENTS"));
    Ok(())
}
