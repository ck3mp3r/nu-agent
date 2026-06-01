use super::persona::{FsPersonaResolver, PersonaError, PersonaFileResolver, FrontMatterParser, PulldownCmarkFrontMatterParser, interpret_front_matter, FrontMatterError, PersonaLister, PersonaSummary};
use std::fs;
use tempfile::TempDir;

#[test]
fn resolve_finds_cwd_file() {
    let temp_cwd = TempDir::new().unwrap();
    let temp_config = TempDir::new().unwrap();

    let agents_dir = temp_cwd.path().join(".agents");
    fs::create_dir_all(&agents_dir).unwrap();
    let persona_file = agents_dir.join("test-agent.md");
    fs::write(&persona_file, "# Test Agent\nContent here").unwrap();

    let resolver = FsPersonaResolver::new(temp_cwd.path().to_path_buf(), temp_config.path().to_path_buf());
    let result = resolver.resolve("test-agent");

    assert!(result.is_ok());
    let (path, content) = result.unwrap();
    assert_eq!(path, persona_file);
    assert_eq!(content, "# Test Agent\nContent here");
}

#[test]
fn resolve_finds_home_file() {
    let temp_cwd = TempDir::new().unwrap();
    let temp_config = TempDir::new().unwrap();

    let agents_dir = temp_config.path().join("agents");
    fs::create_dir_all(&agents_dir).unwrap();
    let persona_file = agents_dir.join("test-agent.md");
    fs::write(&persona_file, "# Home Agent\nHome content").unwrap();

    let resolver = FsPersonaResolver::new(temp_cwd.path().to_path_buf(), temp_config.path().to_path_buf());
    let result = resolver.resolve("test-agent");

    assert!(result.is_ok());
    let (path, content) = result.unwrap();
    assert_eq!(path, persona_file);
    assert_eq!(content, "# Home Agent\nHome content");
}

#[test]
fn resolve_cwd_takes_precedence() {
    let temp_cwd = TempDir::new().unwrap();
    let temp_config = TempDir::new().unwrap();

    // Create persona in cwd
    let cwd_agents_dir = temp_cwd.path().join(".agents");
    fs::create_dir_all(&cwd_agents_dir).unwrap();
    let cwd_persona = cwd_agents_dir.join("test-agent.md");
    fs::write(&cwd_persona, "# CWD Agent").unwrap();

    // Create persona in config
    let config_agents_dir = temp_config.path().join("agents");
    fs::create_dir_all(&config_agents_dir).unwrap();
    let config_persona = config_agents_dir.join("test-agent.md");
    fs::write(&config_persona, "# Config Agent").unwrap();

    let resolver = FsPersonaResolver::new(temp_cwd.path().to_path_buf(), temp_config.path().to_path_buf());
    let result = resolver.resolve("test-agent");

    assert!(result.is_ok());
    let (path, content) = result.unwrap();
    assert_eq!(path, cwd_persona);
    assert_eq!(content, "# CWD Agent");
}

#[test]
fn resolve_not_found_lists_both_paths() {
    let temp_cwd = TempDir::new().unwrap();
    let temp_config = TempDir::new().unwrap();

    let resolver = FsPersonaResolver::new(temp_cwd.path().to_path_buf(), temp_config.path().to_path_buf());
    let result = resolver.resolve("nonexistent");

    assert!(result.is_err());
    if let Err(PersonaError::NotFound { cwd_path, config_path }) = result {
        assert_eq!(cwd_path, temp_cwd.path().join(".agents").join("nonexistent.md"));
        assert_eq!(config_path, temp_config.path().join("agents").join("nonexistent.md"));
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

    let resolver = FsPersonaResolver::new(temp_cwd.path().to_path_buf(), temp_config.path().to_path_buf());
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

// Front matter parser tests

#[test]
fn parse_with_front_matter() {
    let input = r#"---
permissions:
  "*": allow
---
# Body

Content here"#;

    let parser = PulldownCmarkFrontMatterParser;
    let result = parser.parse(input);

    assert!(result.is_ok());
    let parsed = result.unwrap();
    assert!(parsed.front_matter.is_some());
    
    let front_matter = parsed.front_matter.unwrap();
    assert!(front_matter.contains_key("permissions"));
    assert_eq!(parsed.body, "# Body\n\nContent here");
}

#[test]
fn parse_without_front_matter() {
    let input = r#"# Body

Content here"#;

    let parser = PulldownCmarkFrontMatterParser;
    let result = parser.parse(input);

    assert!(result.is_ok());
    let parsed = result.unwrap();
    assert!(parsed.front_matter.is_none());
    assert_eq!(parsed.body, input);
}

#[test]
fn parse_empty_front_matter() {
    // Note: pulldown-cmark treats `---\n---\n` without content as horizontal rules, not metadata.
    // A truly empty YAML block would need at least whitespace or a comment.
    let input = r#"---
# Empty YAML
---
# Body

Content here"#;

    let parser = PulldownCmarkFrontMatterParser;
    let result = parser.parse(input);

    assert!(result.is_ok());
    let parsed = result.unwrap();
    assert!(parsed.front_matter.is_some());
    
    let front_matter = parsed.front_matter.unwrap();
    assert_eq!(front_matter.len(), 0);
    assert_eq!(parsed.body, "# Body\n\nContent here");
}

#[test]
fn parse_invalid_yaml() {
    let input = r#"---
[invalid: yaml:
---
# Body"#;

    let parser = PulldownCmarkFrontMatterParser;
    let result = parser.parse(input);

    assert!(result.is_err());
}

#[test]
fn parse_body_preserves_content() {
    let input = r#"---
key: value
---
# Heading

- List item 1
- List item 2

Code:
```rust
fn main() {}
```"#;

    let parser = PulldownCmarkFrontMatterParser;
    let result = parser.parse(input);

    assert!(result.is_ok());
    let parsed = result.unwrap();
    
    // Body should not have leading/trailing whitespace artifacts
    assert!(parsed.body.starts_with("# Heading"));
    assert!(parsed.body.contains("- List item 1"));
    assert!(parsed.body.contains("fn main() {}"));
}

#[test]
fn parse_multiline_yaml() {
    let input = r#"---
permissions:
  read: allow
  write: deny
author:
  name: Test User
  email: test@example.com
tags:
  - rust
  - testing
---
# Document

Body content"#;

    let parser = PulldownCmarkFrontMatterParser;
    let result = parser.parse(input);

    assert!(result.is_ok());
    let parsed = result.unwrap();
    assert!(parsed.front_matter.is_some());
    
    let front_matter = parsed.front_matter.unwrap();
    assert!(front_matter.contains_key("permissions"));
    assert!(front_matter.contains_key("author"));
    assert!(front_matter.contains_key("tags"));
    assert_eq!(parsed.body, "# Document\n\nBody content");
}

// interpret_front_matter tests

#[test]
fn interpret_name_extracts_string() {
    let mut mapping = noyalib::Mapping::new();
    mapping.insert(
        "name",
        noyalib::Value::String("test-agent".to_string()),
    );

    let result = interpret_front_matter(Some(&mapping), "body content".to_string());
    assert!(result.is_ok());
    let persona = result.unwrap();
    assert_eq!(persona.name, Some("test-agent".to_string()));
    assert_eq!(persona.body, "body content");
}

#[test]
fn interpret_name_rejects_non_string() {
    let mut mapping = noyalib::Mapping::new();
    mapping.insert(
        "name",
        noyalib::Value::Number(42.into()),
    );

    let result = interpret_front_matter(Some(&mapping), "body".to_string());
    assert!(result.is_err());
    if let Err(FrontMatterError::InvalidField { key, expected, got }) = result {
        assert_eq!(key, "name");
        assert_eq!(expected, "string");
        assert_eq!(got, "number");
    } else {
        panic!("Expected InvalidField error");
    }
}

#[test]
fn interpret_description_extracts_string() {
    let mut mapping = noyalib::Mapping::new();
    mapping.insert(
        "description",
        noyalib::Value::String("A test agent".to_string()),
    );

    let result = interpret_front_matter(Some(&mapping), "body".to_string());
    assert!(result.is_ok());
    let persona = result.unwrap();
    assert_eq!(persona.description, Some("A test agent".to_string()));
}

#[test]
fn interpret_model_extracts_string() {
    let mut mapping = noyalib::Mapping::new();
    mapping.insert(
        "model",
        noyalib::Value::String("gpt-4".to_string()),
    );

    let result = interpret_front_matter(Some(&mapping), "body".to_string());
    assert!(result.is_ok());
    let persona = result.unwrap();
    assert_eq!(persona.model, Some("gpt-4".to_string()));
}

#[test]
fn interpret_tool_filter_extracts_list() {
    let mut mapping = noyalib::Mapping::new();
    let tools = vec![
        noyalib::Value::String("read".to_string()),
        noyalib::Value::String("write".to_string()),
    ];
    mapping.insert(
        "tool_filter",
        noyalib::Value::Sequence(tools),
    );

    let result = interpret_front_matter(Some(&mapping), "body".to_string());
    assert!(result.is_ok());
    let persona = result.unwrap();
    assert_eq!(persona.tool_filter, Some(vec!["read".to_string(), "write".to_string()]));
}

#[test]
fn interpret_tool_filter_rejects_non_list() {
    let mut mapping = noyalib::Mapping::new();
    mapping.insert(
        "tool_filter",
        noyalib::Value::String("not-a-list".to_string()),
    );

    let result = interpret_front_matter(Some(&mapping), "body".to_string());
    assert!(result.is_err());
    if let Err(FrontMatterError::InvalidField { key, expected, got }) = result {
        assert_eq!(key, "tool_filter");
        assert_eq!(expected, "sequence");
        assert_eq!(got, "string");
    } else {
        panic!("Expected InvalidField error");
    }
}

#[test]
fn interpret_tool_filter_rejects_non_string_elements() {
    let mut mapping = noyalib::Mapping::new();
    let tools = vec![
        noyalib::Value::String("read".to_string()),
        noyalib::Value::Number(42.into()),
    ];
    mapping.insert(
        "tool_filter",
        noyalib::Value::Sequence(tools),
    );

    let result = interpret_front_matter(Some(&mapping), "body".to_string());
    assert!(result.is_err());
    if let Err(FrontMatterError::InvalidField { key, expected, got }) = result {
        assert_eq!(key, "tool_filter");
        assert_eq!(expected, "sequence of strings");
        assert_eq!(got, "sequence with non-string element");
    } else {
        panic!("Expected InvalidField error");
    }
}

#[test]
fn interpret_permissions_extracts_mapping() {
    let mut mapping = noyalib::Mapping::new();
    let mut perms = noyalib::Mapping::new();
    perms.insert(
        "read",
        noyalib::Value::String("allow".to_string()),
    );
    mapping.insert(
        "permissions",
        noyalib::Value::Mapping(perms.clone()),
    );

    let result = interpret_front_matter(Some(&mapping), "body".to_string());
    assert!(result.is_ok());
    let persona = result.unwrap();
    assert!(persona.permissions.is_some());
    let extracted_perms = persona.permissions.unwrap();
    assert_eq!(extracted_perms, perms);
}

#[test]
fn interpret_no_front_matter() {
    let result = interpret_front_matter(None, "body content".to_string());
    assert!(result.is_ok());
    let persona = result.unwrap();
    assert_eq!(persona.name, None);
    assert_eq!(persona.description, None);
    assert_eq!(persona.model, None);
    assert_eq!(persona.tool_filter, None);
    assert_eq!(persona.permissions, None);
    assert_eq!(persona.body, "body content");
}

#[test]
fn interpret_unknown_keys_ignored() {
    let mut mapping = noyalib::Mapping::new();
    mapping.insert(
        "name",
        noyalib::Value::String("test".to_string()),
    );
    mapping.insert(
        "unknown_key",
        noyalib::Value::String("ignored".to_string()),
    );
    mapping.insert(
        "another_unknown",
        noyalib::Value::Number(42.into()),
    );

    let result = interpret_front_matter(Some(&mapping), "body".to_string());
    assert!(result.is_ok());
    let persona = result.unwrap();
    assert_eq!(persona.name, Some("test".to_string()));
    assert_eq!(persona.description, None);
}

#[test]
fn interpret_all_fields_together() {
    let mut mapping = noyalib::Mapping::new();
    mapping.insert(
        "name",
        noyalib::Value::String("full-agent".to_string()),
    );
    mapping.insert(
        "description",
        noyalib::Value::String("A complete agent".to_string()),
    );
    mapping.insert(
        "model",
        noyalib::Value::String("claude-3".to_string()),
    );
    let tools = vec![
        noyalib::Value::String("read".to_string()),
        noyalib::Value::String("write".to_string()),
    ];
    mapping.insert(
        "tool_filter",
        noyalib::Value::Sequence(tools),
    );
    let mut perms = noyalib::Mapping::new();
    perms.insert(
        "*",
        noyalib::Value::String("allow".to_string()),
    );
    mapping.insert(
        "permissions",
        noyalib::Value::Mapping(perms.clone()),
    );

    let result = interpret_front_matter(Some(&mapping), "body content".to_string());
    assert!(result.is_ok());
    let persona = result.unwrap();
    assert_eq!(persona.name, Some("full-agent".to_string()));
    assert_eq!(persona.description, Some("A complete agent".to_string()));
    assert_eq!(persona.model, Some("claude-3".to_string()));
    assert_eq!(persona.tool_filter, Some(vec!["read".to_string(), "write".to_string()]));
    assert!(persona.permissions.is_some());
    assert_eq!(persona.body, "body content");
}

#[test]
fn interpret_empty_mapping() {
    let mapping = noyalib::Mapping::new();

    let result = interpret_front_matter(Some(&mapping), "body".to_string());
    assert!(result.is_ok());
    let persona = result.unwrap();
    assert_eq!(persona.name, None);
    assert_eq!(persona.description, None);
    assert_eq!(persona.model, None);
    assert_eq!(persona.tool_filter, None);
    assert_eq!(persona.permissions, None);
    assert_eq!(persona.body, "body");
}

#[test]
fn list_available_returns_empty_when_no_dirs() {
    let temp_cwd = TempDir::new().unwrap();
    let temp_config = TempDir::new().unwrap();
    let resolver = FsPersonaResolver::new(temp_cwd.path().to_path_buf(), temp_config.path().to_path_buf());
    let result = resolver.list_available();
    assert!(result.is_empty());
}

#[test]
fn list_available_finds_cwd_agents() {
    let temp_cwd = TempDir::new().unwrap();
    let temp_config = TempDir::new().unwrap();

    let agents_dir = temp_cwd.path().join(".agents");
    fs::create_dir_all(&agents_dir).unwrap();
    fs::write(
        agents_dir.join("coder.md"),
        "---\nname: coder\ndescription: Writes code\n---\n# Coder",
    ).unwrap();

    let resolver = FsPersonaResolver::new(temp_cwd.path().to_path_buf(), temp_config.path().to_path_buf());
    let result = resolver.list_available();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].name, "coder");
    assert_eq!(result[0].description.as_deref(), Some("Writes code"));
}

#[test]
fn list_available_deduplicates_cwd_over_xdg() {
    let temp_cwd = TempDir::new().unwrap();
    let temp_config = TempDir::new().unwrap();

    let cwd_agents = temp_cwd.path().join(".agents");
    fs::create_dir_all(&cwd_agents).unwrap();
    fs::write(cwd_agents.join("coder.md"), "---\nname: local-coder\ndescription: Local\n---\n").unwrap();

    let xdg_agents = temp_config.path().join("agents");
    fs::create_dir_all(&xdg_agents).unwrap();
    fs::write(xdg_agents.join("coder.md"), "---\nname: global-coder\ndescription: Global\n---\n").unwrap();

    let resolver = FsPersonaResolver::new(temp_cwd.path().to_path_buf(), temp_config.path().to_path_buf());
    let result = resolver.list_available();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].name, "local-coder");
    assert_eq!(result[0].description.as_deref(), Some("Local"));
}

#[test]
fn list_available_merges_cwd_and_xdg() {
    let temp_cwd = TempDir::new().unwrap();
    let temp_config = TempDir::new().unwrap();

    let cwd_agents = temp_cwd.path().join(".agents");
    fs::create_dir_all(&cwd_agents).unwrap();
    fs::write(cwd_agents.join("coder.md"), "---\nname: coder\n---\n").unwrap();

    let xdg_agents = temp_config.path().join("agents");
    fs::create_dir_all(&xdg_agents).unwrap();
    fs::write(xdg_agents.join("reviewer.md"), "---\nname: reviewer\n---\n").unwrap();

    let resolver = FsPersonaResolver::new(temp_cwd.path().to_path_buf(), temp_config.path().to_path_buf());
    let result = resolver.list_available();
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].name, "coder");
    assert_eq!(result[1].name, "reviewer");
}

#[test]
fn list_available_uses_filename_stem_when_no_name_in_frontmatter() {
    let temp_cwd = TempDir::new().unwrap();
    let temp_config = TempDir::new().unwrap();

    let agents_dir = temp_cwd.path().join(".agents");
    fs::create_dir_all(&agents_dir).unwrap();
    fs::write(agents_dir.join("my-agent.md"), "# Just a body, no front matter").unwrap();

    let resolver = FsPersonaResolver::new(temp_cwd.path().to_path_buf(), temp_config.path().to_path_buf());
    let result = resolver.list_available();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].name, "my-agent");
    assert_eq!(result[0].description, None);
}

#[test]
fn list_available_skips_non_md_files() {
    let temp_cwd = TempDir::new().unwrap();
    let temp_config = TempDir::new().unwrap();

    let agents_dir = temp_cwd.path().join(".agents");
    fs::create_dir_all(&agents_dir).unwrap();
    fs::write(agents_dir.join("coder.md"), "---\nname: coder\n---\n").unwrap();
    fs::write(agents_dir.join("notes.txt"), "not a persona").unwrap();
    fs::write(agents_dir.join("config.yaml"), "key: value").unwrap();

    let resolver = FsPersonaResolver::new(temp_cwd.path().to_path_buf(), temp_config.path().to_path_buf());
    let result = resolver.list_available();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].name, "coder");
}
