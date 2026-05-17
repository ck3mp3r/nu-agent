use std::fs;

use tempfile::tempdir;

use super::skills::{
    SkillResolveError, SkillSource, discover_skill_catalog_for_cwd_for_tests,
    extract_skill_description, is_higher_precedence_for_tests,
    render_available_skills_preamble_for_tests, resolve_explicit_skill_request_for_cwd_for_tests,
};

#[test]
fn discovers_home_skills_in_catalog() {
    let tmp = tempdir().expect("tempdir");
    let home = tmp.path().join("home");
    let cwd = tmp.path().join("repo");

    fs::create_dir_all(home.join(".agents/skills/nushell-shell")).expect("home skill dir");
    fs::write(
        home.join(".agents/skills/nushell-shell/SKILL.md"),
        "home skill content\n",
    )
    .expect("home skill");
    fs::create_dir_all(&cwd).expect("cwd");

    let catalog = discover_skill_catalog_for_cwd_for_tests(&cwd, Some(&home), Some(tmp.path()));

    assert!(
        catalog
            .iter()
            .any(|entry| { entry.name == "nushell-shell" && entry.source == SkillSource::Home })
    );
}

#[test]
fn resolves_explicit_skill_request_from_home_source() {
    let tmp = tempdir().expect("tempdir");
    let home = tmp.path().join("home");
    let cwd = tmp.path().join("repo");

    fs::create_dir_all(home.join(".agents/skills/context")).expect("home skill dir");
    fs::write(
        home.join(".agents/skills/context/SKILL.md"),
        "home context skill\n",
    )
    .expect("home skill");
    fs::create_dir_all(&cwd).expect("cwd");

    let resolved = resolve_explicit_skill_request_for_cwd_for_tests(
        &cwd,
        Some(&home),
        Some(tmp.path()),
        "context",
    )
    .expect("resolve should succeed")
    .expect("skill must resolve");

    assert_eq!(resolved.source, SkillSource::Home);
    assert_eq!(resolved.content, "home context skill\n");
}

#[test]
fn local_source_wins_on_skill_name_collision() {
    let tmp = tempdir().expect("tempdir");
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");

    fs::create_dir_all(home.join(".agents/skills/context")).expect("home skill dir");
    fs::write(home.join(".agents/skills/context/SKILL.md"), "home\n").expect("home skill");

    fs::create_dir_all(repo.join(".agents/skills/context")).expect("local skill dir");
    fs::write(repo.join(".agents/skills/context/SKILL.md"), "local\n").expect("local skill");

    let resolved = resolve_explicit_skill_request_for_cwd_for_tests(
        &repo,
        Some(&home),
        Some(tmp.path()),
        "context",
    )
    .expect("resolve should succeed")
    .expect("skill must resolve");

    assert_eq!(resolved.source, SkillSource::Local);
    assert_eq!(resolved.content, "local\n");
}

#[test]
fn rejects_path_traversal_skill_lookup() {
    let tmp = tempdir().expect("tempdir");
    let home = tmp.path().join("home");
    let cwd = tmp.path().join("repo");
    fs::create_dir_all(home.join(".agents/skills/context")).expect("home skill dir");
    fs::write(home.join(".agents/skills/context/SKILL.md"), "home\n").expect("home skill");
    fs::create_dir_all(&cwd).expect("cwd");

    let err = resolve_explicit_skill_request_for_cwd_for_tests(
        &cwd,
        Some(&home),
        Some(tmp.path()),
        "../context",
    )
    .expect_err("traversal must be rejected");

    assert!(matches!(err, SkillResolveError::InvalidSkillName(_)));
}

#[test]
fn rejects_symlink_escape_outside_home_skills_root() {
    let tmp = tempdir().expect("tempdir");
    let home = tmp.path().join("home");
    let cwd = tmp.path().join("repo");
    let outside = tmp.path().join("outside");

    fs::create_dir_all(home.join(".agents/skills")).expect("home skills root");
    fs::create_dir_all(&outside).expect("outside");
    fs::create_dir_all(&cwd).expect("cwd");

    fs::write(outside.join("SKILL.md"), "outside\n").expect("outside skill");

    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, home.join(".agents/skills/escaped")).expect("symlink");

    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&outside, home.join(".agents/skills/escaped"))
        .expect("symlink");

    let err = resolve_explicit_skill_request_for_cwd_for_tests(
        &cwd,
        Some(&home),
        Some(tmp.path()),
        "escaped",
    )
    .expect_err("symlink escape must be rejected");

    assert!(matches!(
        err,
        SkillResolveError::HomeSkillEscapesRoot { skill_name } if skill_name == "escaped"
    ));
}

#[test]
fn missing_skill_preserves_not_found_semantics() {
    let tmp = tempdir().expect("tempdir");
    let home = tmp.path().join("home");
    let cwd = tmp.path().join("repo");
    fs::create_dir_all(home.join(".agents/skills")).expect("home skills root");
    fs::create_dir_all(&cwd).expect("cwd");

    let resolved = resolve_explicit_skill_request_for_cwd_for_tests(
        &cwd,
        Some(&home),
        Some(tmp.path()),
        "does-not-exist",
    )
    .expect("missing skill should not error");

    assert_eq!(resolved, None);
}

#[test]
fn precedence_ordering_handles_deep_ancestry_rank_without_packing_collision() {
    let local_deep = (SkillSource::Local.priority(), 16);
    let home = (SkillSource::Home.priority(), 0);

    assert!(
        is_higher_precedence_for_tests(local_deep, home),
        "local source precedence must remain stable even when ancestry rank >= 16"
    );
    assert!(
        !is_higher_precedence_for_tests(home, local_deep),
        "distinct precedence tuples must not collapse into an equal rank"
    );
}

#[test]
fn available_skills_preamble_renders_catalog_entries() {
    let tmp = tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(repo.join(".agents/skills/context")).expect("local skills dir");
    fs::write(repo.join(".agents/skills/context/SKILL.md"), "context\n").expect("skill file");

    let preamble = render_available_skills_preamble_for_tests(&repo, None, Some(tmp.path()))
        .expect("preamble should render");

    assert!(preamble.contains("<available_skills>"));
    assert!(preamble.contains("<name>context</name>"));
    assert!(preamble.contains("<source>local</source>"));
}

#[test]
fn available_skills_preamble_absent_when_catalog_empty() {
    let tmp = tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(&repo).expect("repo dir");

    let preamble = render_available_skills_preamble_for_tests(&repo, None, Some(tmp.path()));
    assert!(preamble.is_none());
}

#[test]
fn skill_preamble_xml_structure_with_description() {
    let tmp = tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(repo.join(".agents/skills/example")).expect("local skills dir");
    fs::write(
        repo.join(".agents/skills/example/SKILL.md"),
        "# Example Skill\n\nThis is an example description.\n",
    )
    .expect("skill file");

    let preamble = render_available_skills_preamble_for_tests(&repo, None, Some(tmp.path()))
        .expect("preamble should render");

    // Verify the XML structure includes description between name and source
    let expected_structure = r#"  <skill>
    <name>example</name>
    <description>This is an example description.</description>
    <source>local</source>
  </skill>"#;

    assert!(preamble.contains(expected_structure));
}

#[test]
fn extract_skill_description_from_first_non_heading_line() {
    let content = r#"# Skill: nushell-shell

# Nushell Shell Patterns

This skill covers using Nushell as a shell, including redirection.

More content here.
"#;
    let desc = extract_skill_description(content).expect("should extract description");
    assert_eq!(
        desc,
        "This skill covers using Nushell as a shell, including redirection."
    );
}

#[test]
fn extract_skill_description_truncates_long_lines() {
    let long_line = "a".repeat(200);
    let content = format!("# Heading\n\n{long_line}\n");
    let desc = extract_skill_description(&content).expect("should extract description");
    assert_eq!(desc.len(), 153); // 150 + '…' (3 bytes in UTF-8)
    assert!(desc.ends_with('…'));
}

#[test]
fn extract_skill_description_returns_none_when_only_headings() {
    let content = "# Heading 1\n## Heading 2\n### Heading 3\n";
    let desc = extract_skill_description(content);
    assert!(desc.is_none());
}

#[test]
fn extract_skill_description_skips_empty_lines() {
    let content = "# Heading\n\n\n\nActual description here.\n";
    let desc = extract_skill_description(content).expect("should extract description");
    assert_eq!(desc, "Actual description here.");
}

#[test]
fn available_skills_preamble_includes_descriptions() {
    let tmp = tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(repo.join(".agents/skills/context")).expect("local skills dir");
    fs::write(
        repo.join(".agents/skills/context/SKILL.md"),
        "# Context Skill\n\nManage context effectively.\n",
    )
    .expect("skill file");

    let preamble = render_available_skills_preamble_for_tests(&repo, None, Some(tmp.path()))
        .expect("preamble should render");

    assert!(preamble.contains("<available_skills>"));
    assert!(preamble.contains("<name>context</name>"));
    assert!(preamble.contains("<description>Manage context effectively.</description>"));
    assert!(preamble.contains("<source>local</source>"));
}

#[test]
fn available_skills_preamble_works_without_descriptions() {
    let tmp = tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(repo.join(".agents/skills/empty")).expect("local skills dir");
    fs::write(
        repo.join(".agents/skills/empty/SKILL.md"),
        "# Only heading\n",
    )
    .expect("skill file");

    let preamble = render_available_skills_preamble_for_tests(&repo, None, Some(tmp.path()))
        .expect("preamble should render");

    assert!(preamble.contains("<name>empty</name>"));
    assert!(!preamble.contains("<description>"));
}
