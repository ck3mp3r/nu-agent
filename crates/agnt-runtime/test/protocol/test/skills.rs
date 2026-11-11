use super::*;

// === Tests: skills ===

#[test]
fn discover_and_resolve_skill_scenarios() -> Result<()> {
    // Home skills are discovered in the catalog.
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let cwd = tmp.path().join("repo");

    fs::create_dir_all(home.join(".agents/skills/nushell-shell"))?;
    fs::write(
        home.join(".agents/skills/nushell-shell/SKILL.md"),
        "home skill content\n",
    )?;
    fs::create_dir_all(&cwd)?;

    let catalog = discover_skill_catalog(&cwd, Some(&home), Some(tmp.path()));

    assert!(
        catalog
            .iter()
            .any(|entry| { entry.name == "nushell-shell" && entry.source == SkillSource::Home })
    );

    // An explicit request resolves from the home source.
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let cwd = tmp.path().join("repo");

    fs::create_dir_all(home.join(".agents/skills/context"))?;
    fs::write(
        home.join(".agents/skills/context/SKILL.md"),
        "home context skill\n",
    )?;
    fs::create_dir_all(&cwd)?;

    let resolved = resolve_explicit_skill_request(&cwd, Some(&home), Some(tmp.path()), "context")
        .map_err(|e| format!("resolve should succeed: {e:?}"))?
        .ok_or("skill must resolve")?;

    assert_eq!(resolved.source, SkillSource::Home);
    assert_eq!(resolved.content, "home context skill\n");

    // The local source wins on a skill-name collision.
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");

    fs::create_dir_all(home.join(".agents/skills/context"))?;
    fs::write(home.join(".agents/skills/context/SKILL.md"), "home\n")?;

    fs::create_dir_all(repo.join(".agents/skills/context"))?;
    fs::write(repo.join(".agents/skills/context/SKILL.md"), "local\n")?;

    let resolved = resolve_explicit_skill_request(&repo, Some(&home), Some(tmp.path()), "context")
        .map_err(|e| format!("resolve should succeed: {e:?}"))?
        .ok_or("skill must resolve")?;

    assert_eq!(resolved.source, SkillSource::Local);
    assert_eq!(resolved.content, "local\n");

    // A missing skill preserves not-found semantics.
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let cwd = tmp.path().join("repo");
    fs::create_dir_all(home.join(".agents/skills"))?;
    fs::create_dir_all(&cwd)?;

    let resolved =
        resolve_explicit_skill_request(&cwd, Some(&home), Some(tmp.path()), "does-not-exist")
            .map_err(|e| format!("missing skill should not error: {e:?}"))?;

    assert_eq!(resolved, None);
    Ok(())
}

#[test]
fn rejects_path_traversal_skill_lookup() -> Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let cwd = tmp.path().join("repo");
    fs::create_dir_all(home.join(".agents/skills/context"))?;
    fs::write(home.join(".agents/skills/context/SKILL.md"), "home\n")?;
    fs::create_dir_all(&cwd)?;

    let err =
        match resolve_explicit_skill_request(&cwd, Some(&home), Some(tmp.path()), "../context") {
            Ok(_) => return Err("traversal must be rejected".into()),
            Err(e) => e,
        };

    assert!(matches!(err, SkillResolveError::InvalidSkillName(_)));
    Ok(())
}

#[test]
fn rejects_symlink_escape_outside_home_skills_root() -> Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let cwd = tmp.path().join("repo");
    let outside = tmp.path().join("outside");

    fs::create_dir_all(home.join(".agents/skills"))?;
    fs::create_dir_all(&outside)?;
    fs::create_dir_all(&cwd)?;

    fs::write(outside.join("SKILL.md"), "outside\n")?;

    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, home.join(".agents/skills/escaped"))?;

    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&outside, home.join(".agents/skills/escaped"))?;

    let err = match resolve_explicit_skill_request(&cwd, Some(&home), Some(tmp.path()), "escaped") {
        Ok(_) => return Err("symlink escape must be rejected".into()),
        Err(e) => e,
    };

    assert!(matches!(
        err,
        SkillResolveError::HomeSkillEscapesRoot { skill_name } if skill_name == "escaped"
    ));
    Ok(())
}

#[test]
fn available_skills_preamble_scenarios() -> Result<()> {
    // Catalog entries render into the preamble.
    let tmp = tempdir()?;
    let repo = tmp.path().join("repo");
    fs::create_dir_all(repo.join(".agents/skills/context"))?;
    fs::write(repo.join(".agents/skills/context/SKILL.md"), "context\n")?;

    let preamble = render_available_skills_preamble_from_catalog(discover_skill_catalog(
        &repo,
        None,
        Some(tmp.path()),
    ))
    .ok_or("preamble should render")?;

    assert!(preamble.contains("<available_skills>"));
    assert!(preamble.contains("<name>context</name>"));
    assert!(preamble.contains("<source>local</source>"));

    // An empty catalog renders no preamble.
    let tmp = tempdir()?;
    let repo = tmp.path().join("repo");
    fs::create_dir_all(&repo)?;

    let preamble = render_available_skills_preamble_from_catalog(discover_skill_catalog(
        &repo,
        None,
        Some(tmp.path()),
    ));
    assert!(preamble.is_none());

    // The XML structure places the description between name and source.
    let tmp = tempdir()?;
    let repo = tmp.path().join("repo");
    fs::create_dir_all(repo.join(".agents/skills/example"))?;
    fs::write(
        repo.join(".agents/skills/example/SKILL.md"),
        "# Example Skill\n\nThis is an example description.\n",
    )?;

    let preamble = render_available_skills_preamble_from_catalog(discover_skill_catalog(
        &repo,
        None,
        Some(tmp.path()),
    ))
    .ok_or("preamble should render")?;

    let expected_structure = r#"  <skill>
    <name>example</name>
    <description>This is an example description.</description>
    <source>local</source>
  </skill>"#;

    assert!(preamble.contains(expected_structure));

    // Descriptions are included when present.
    let tmp = tempdir()?;
    let repo = tmp.path().join("repo");
    fs::create_dir_all(repo.join(".agents/skills/context"))?;
    fs::write(
        repo.join(".agents/skills/context/SKILL.md"),
        "# Context Skill\n\nManage context effectively.\n",
    )?;

    let preamble = render_available_skills_preamble_from_catalog(discover_skill_catalog(
        &repo,
        None,
        Some(tmp.path()),
    ))
    .ok_or("preamble should render")?;

    assert!(preamble.contains("<available_skills>"));
    assert!(preamble.contains("<name>context</name>"));
    assert!(preamble.contains("<description>Manage context effectively.</description>"));
    assert!(preamble.contains("<source>local</source>"));

    // The preamble works without descriptions.
    let tmp = tempdir()?;
    let repo = tmp.path().join("repo");
    fs::create_dir_all(repo.join(".agents/skills/empty"))?;
    fs::write(
        repo.join(".agents/skills/empty/SKILL.md"),
        "# Only heading\n",
    )?;

    let preamble = render_available_skills_preamble_from_catalog(discover_skill_catalog(
        &repo,
        None,
        Some(tmp.path()),
    ))
    .ok_or("preamble should render")?;

    assert!(preamble.contains("<name>empty</name>"));
    assert!(!preamble.contains("<description>"));
    Ok(())
}

#[test]
fn extract_skill_description_table() -> Result<()> {
    // -- Setup & Fixtures
    let long_line = "a".repeat(200);
    let multibyte = format!("{}é rest of the description", "a".repeat(149));
    let cases: Vec<(String, Option<String>)> = vec![
        (
            "# Skill: nushell-shell\n\n# Nushell Shell Patterns\n\nThis skill covers using Nushell as a shell, including redirection.\n\nMore content here.\n".to_string(),
            Some("This skill covers using Nushell as a shell, including redirection.".to_string()),
        ),
        (
            format!("# Heading\n\n{long_line}\n"),
            Some(format!("{}…", "a".repeat(150))),
        ),
        (
            format!("---\ndescription: \"{multibyte}\"\n---\n"),
            Some(format!("{}…", "a".repeat(149))),
        ),
        (
            "# Heading 1\n## Heading 2\n### Heading 3\n".to_string(),
            None,
        ),
        (
            "# Heading\n\n\n\nActual description here.\n".to_string(),
            Some("Actual description here.".to_string()),
        ),
        (
            "---\nname: context\ndescription: Working effectively with c5t.\nlicense: GPL-2.0\n---\n\n# c5t Context Management\n\nc5t is a personal context manager.\n".to_string(),
            Some("Working effectively with c5t.".to_string()),
        ),
        (
            "---\nname: nushell\nlicense: GPL-2.0\n---\n\n# Nushell Guide\n\nBody description here.\n".to_string(),
            Some("Body description here.".to_string()),
        ),
        (
            "---\ndescription: \"Use Nushell as a shell.\"\n---\n\n# Heading\n".to_string(),
            Some("Use Nushell as a shell.".to_string()),
        ),
    ];

    // -- Exec & Check
    for (content, expected) in &cases {
        assert_eq!(
            extract_skill_description(content).as_deref(),
            expected.as_deref(),
            "content={content}"
        );
    }
    Ok(())
}
