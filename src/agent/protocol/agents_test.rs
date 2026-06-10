use std::fs;

use tempfile::tempdir;

use super::agents::load_agents_chain_for_cwd_for_tests;

#[test]
fn loads_home_agents_when_present() {
    let tmp = tempdir().expect("tempdir");
    let config = tmp.path().join("config");
    let cwd = tmp.path().join("cwd");
    fs::create_dir_all(config.join("agents")).expect("config/agents");
    fs::create_dir_all(&cwd).expect("cwd");
    fs::write(config.join("agents/AGENTS.md"), "CONFIG\n").expect("write config agents");

    let loaded = load_agents_chain_for_cwd_for_tests(&cwd, Some(&config), Some(tmp.path()), None);

    assert_eq!(loaded.merged_chain.as_deref(), Some("CONFIG\n"));
    assert!(loaded.warnings.is_empty());
}

#[test]
fn loads_cwd_agents_when_present() {
    let tmp = tempdir().expect("tempdir");
    let cwd = tmp.path().join("cwd");
    fs::create_dir_all(&cwd).expect("cwd");
    fs::write(cwd.join("AGENTS.md"), "CWD\n").expect("write cwd agents");

    let loaded = load_agents_chain_for_cwd_for_tests(&cwd, None, Some(tmp.path()), None);

    assert_eq!(loaded.merged_chain.as_deref(), Some("CWD\n"));
    assert!(loaded.warnings.is_empty());
}

#[test]
fn loads_ancestor_agents_in_root_to_leaf_order() {
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path().join("root");
    let parent = root.join("a");
    let cwd = parent.join("b");
    fs::create_dir_all(&cwd).expect("cwd");
    fs::write(root.join("AGENTS.md"), "ROOT\n").expect("root agents");
    fs::write(parent.join("AGENTS.md"), "PARENT\n").expect("parent agents");

    let loaded = load_agents_chain_for_cwd_for_tests(&cwd, None, Some(&root), None);

    assert_eq!(loaded.merged_chain.as_deref(), Some("ROOT\n\nPARENT\n"));
    assert!(loaded.warnings.is_empty());
}

#[test]
fn nearest_agents_has_highest_precedence_position() {
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path().join("root");
    let cwd = root.join("child");
    fs::create_dir_all(&cwd).expect("cwd");
    fs::write(root.join("AGENTS.md"), "ROOT\n").expect("root agents");
    fs::write(cwd.join("AGENTS.md"), "CWD\n").expect("cwd agents");

    let loaded = load_agents_chain_for_cwd_for_tests(&cwd, None, Some(&root), None);
    let merged = loaded.merged_chain.expect("merged chain");

    assert!(merged.ends_with("CWD\n"));
    assert!(merged.contains("ROOT\n"));
}

#[test]
fn missing_home_or_agents_files_is_noop() {
    let tmp = tempdir().expect("tempdir");
    let cwd = tmp.path().join("cwd");
    fs::create_dir_all(&cwd).expect("cwd");

    let loaded = load_agents_chain_for_cwd_for_tests(&cwd, None, Some(tmp.path()), None);

    assert_eq!(loaded.merged_chain, None);
    assert!(loaded.warnings.is_empty());
}

#[test]
fn unreadable_agents_is_non_fatal() {
    let tmp = tempdir().expect("tempdir");
    let cwd = tmp.path().join("cwd");
    let agents = cwd.join("AGENTS.md");
    fs::create_dir_all(&agents).expect("create AGENTS.md directory to force read error");

    let loaded = load_agents_chain_for_cwd_for_tests(&cwd, None, Some(tmp.path()), None);

    assert!(loaded.warnings.iter().any(|w| w.contains("AGENTS.md")));
    assert_eq!(loaded.merged_chain, None);
}

#[test]
fn canonical_path_dedup_prevents_duplicate_load() {
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path().join("root");
    let real = root.join("real");
    let alias = root.join("alias");
    fs::create_dir_all(&real).expect("real");
    fs::write(real.join("AGENTS.md"), "REAL\n").expect("write agents");

    #[cfg(unix)]
    std::os::unix::fs::symlink(&real, &alias).expect("symlink alias->real");

    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&real, &alias).expect("symlink alias->real");

    let cwd = alias.join("child");
    fs::create_dir_all(&cwd).expect("cwd");

    let loaded = load_agents_chain_for_cwd_for_tests(&cwd, None, Some(&root), None);
    let merged = loaded.merged_chain.expect("merged chain");

    assert_eq!(merged.matches("REAL").count(), 1);
}

#[test]
fn loads_from_home_dot_agents() {
    let tmp = tempdir().expect("tempdir");
    let home = tmp.path().join("home");
    let cwd = tmp.path().join("cwd");
    fs::create_dir_all(home.join(".agents")).expect("create .agents");
    fs::create_dir_all(&cwd).expect("create cwd");
    fs::write(home.join(".agents/AGENTS.md"), "HOME_AGENTS\n").expect("write");

    let result =
        load_agents_chain_for_cwd_for_tests(&cwd, None, Some(tmp.path()), Some(&home));
    assert!(result.merged_chain.unwrap().contains("HOME_AGENTS"));
}
