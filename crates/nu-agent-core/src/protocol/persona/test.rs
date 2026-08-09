use super::{
    FrontMatterError, FrontMatterParser, FsPersonaResolver, PersonaError, PersonaFileResolver,
    PersonaLister, PulldownCmarkFrontMatterParser, interpret_front_matter,
};
use crate::config::AgentsConfig;
use crate::protocol::persona::builtins::BUILTIN_PLANNER_CONTENT;
use std::fs;
use tempfile::TempDir;

/// AgentsConfig with all builtins disabled (for filesystem-only tests)
fn no_builtins() -> AgentsConfig {
    AgentsConfig {
        planner_enabled: false,
        maker_enabled: false,
        ..AgentsConfig::default()
    }
}

#[test]
fn resolve_finds_cwd_file() {
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

    let resolver = FsPersonaResolver::new(
        temp_cwd.path().to_path_buf(),
        temp_config.path().to_path_buf(),
        AgentsConfig::default(),
    );
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

    let resolver = FsPersonaResolver::new(
        temp_cwd.path().to_path_buf(),
        temp_config.path().to_path_buf(),
        AgentsConfig::default(),
    );
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
    mapping.insert("name", noyalib::Value::String("test-agent".to_string()));

    let result = interpret_front_matter(Some(&mapping), "body content".to_string());
    assert!(result.is_ok());
    let persona = result.unwrap();
    assert_eq!(persona.name, Some("test-agent".to_string()));
    assert_eq!(persona.body, "body content");
}

#[test]
fn interpret_name_rejects_non_string() {
    let mut mapping = noyalib::Mapping::new();
    mapping.insert("name", noyalib::Value::Number(42.into()));

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
    mapping.insert("model", noyalib::Value::String("gpt-4".to_string()));

    let result = interpret_front_matter(Some(&mapping), "body".to_string());
    assert!(result.is_ok());
    let persona = result.unwrap();
    assert_eq!(persona.model, Some("gpt-4".to_string()));
}

#[test]
fn interpret_permissions_extracts_mapping() {
    let mut mapping = noyalib::Mapping::new();
    let mut perms = noyalib::Mapping::new();
    perms.insert("read", noyalib::Value::String("allow".to_string()));
    mapping.insert("permissions", noyalib::Value::Mapping(perms.clone()));

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
    assert_eq!(persona.permissions, None);
    assert_eq!(persona.body, "body content");
}

#[test]
fn interpret_unknown_keys_ignored() {
    let mut mapping = noyalib::Mapping::new();
    mapping.insert("name", noyalib::Value::String("test".to_string()));
    mapping.insert("unknown_key", noyalib::Value::String("ignored".to_string()));
    mapping.insert("another_unknown", noyalib::Value::Number(42.into()));

    let result = interpret_front_matter(Some(&mapping), "body".to_string());
    assert!(result.is_ok());
    let persona = result.unwrap();
    assert_eq!(persona.name, Some("test".to_string()));
    assert_eq!(persona.description, None);
}

#[test]
fn interpret_all_new_fields_parsed() {
    let mut mapping = noyalib::Mapping::new();
    mapping.insert(
        "temperature",
        noyalib::Value::Number(noyalib::Number::Float(0.7)),
    );
    mapping.insert(
        "max_tokens",
        noyalib::Value::Number(noyalib::Number::Integer(2048)),
    );
    mapping.insert(
        "max_tool_turns",
        noyalib::Value::Number(noyalib::Number::Integer(10)),
    );
    mapping.insert(
        "max_tool_calls_per_subturn",
        noyalib::Value::Number(noyalib::Number::Integer(5)),
    );
    mapping.insert(
        "max_tool_result_bytes",
        noyalib::Value::Number(noyalib::Number::Integer(10000)),
    );
    let mut params = noyalib::Mapping::new();
    params.insert("thinking", noyalib::Value::String("enabled".to_string()));
    mapping.insert("additional_params", noyalib::Value::Mapping(params));

    let persona = interpret_front_matter(Some(&mapping), "body".to_string())
        .expect("should parse all new fields");

    assert_eq!(persona.temperature, Some(0.7));
    assert_eq!(persona.max_tokens, Some(2048));
    assert_eq!(persona.max_tool_turns, Some(10));
    assert_eq!(persona.max_tool_calls_per_subturn, Some(5));
    assert_eq!(persona.max_tool_result_bytes, Some(10000));
    assert!(persona.additional_params.is_some());
    assert_eq!(
        persona.additional_params.as_ref().unwrap()["thinking"],
        "enabled"
    );
}

#[test]
fn interpret_new_fields_wrong_type_errors() {
    let bad_cases: &[(&str, noyalib::Value)] = &[
        ("temperature", noyalib::Value::String("hot".to_string())),
        ("max_tokens", noyalib::Value::String("many".to_string())),
        ("max_tool_turns", noyalib::Value::String("lots".to_string())),
        (
            "max_tool_calls_per_subturn",
            noyalib::Value::String("several".to_string()),
        ),
        (
            "max_tool_result_bytes",
            noyalib::Value::String("big".to_string()),
        ),
        (
            "additional_params",
            noyalib::Value::String("not-a-map".to_string()),
        ),
    ];

    for (key, bad_value) in bad_cases {
        let mut mapping = noyalib::Mapping::new();
        mapping.insert(*key, bad_value.clone());
        let result = interpret_front_matter(Some(&mapping), "body".to_string());
        assert!(result.is_err(), "expected error for key={key}, got ok");
        if let Err(FrontMatterError::InvalidField { key: k, .. }) = result {
            assert_eq!(&k, key, "wrong key in error for {key}");
        } else {
            panic!("expected InvalidField error for key={key}, got different error type");
        }
    }
}

#[test]
fn interpret_integer_fields_reject_negatives() {
    let integer_fields = [
        "max_tokens",
        "max_tool_turns",
        "max_tool_calls_per_subturn",
        "max_tool_result_bytes",
    ];
    for field in &integer_fields {
        let mut mapping = noyalib::Mapping::new();
        mapping.insert(*field, noyalib::Value::Number(noyalib::Number::Integer(-1)));
        let result = interpret_front_matter(Some(&mapping), "body".to_string());
        assert!(
            result.is_err(),
            "expected error for negative {field}, got ok"
        );
    }
}

#[test]
fn interpret_no_front_matter_new_fields_none() {
    let persona = interpret_front_matter(None, "body".to_string())
        .expect("no-front-matter path should succeed");
    assert_eq!(persona.temperature, None);
    assert_eq!(persona.max_tokens, None);
    assert_eq!(persona.max_tool_turns, None);
    assert_eq!(persona.max_tool_calls_per_subturn, None);
    assert_eq!(persona.max_tool_result_bytes, None);
    assert_eq!(persona.additional_params, None);
}

#[test]
fn parse_full_front_matter_with_config_fields() {
    // End-to-end: parse a persona Markdown string through
    // PulldownCmarkFrontMatterParser + interpret_front_matter.
    // Tests the flat numeric fields; additional_params (nested YAML) is
    // covered by interpret_all_new_fields_parsed which constructs the
    // noyalib Mapping directly.
    let input = "---\n\
name: coder\n\
description: A focused coding agent\n\
model: openai/gpt-4o\n\
temperature: 0.2\n\
max_tokens: 8192\n\
max_tool_turns: 20\n\
max_tool_calls_per_subturn: 3\n\
max_tool_result_bytes: 50000\n\
---\n\
\n\
You are a focused coding agent.\n";

    let parser = PulldownCmarkFrontMatterParser;
    let raw = parser.parse(input).expect("parse should succeed");
    let persona = interpret_front_matter(raw.front_matter.as_ref(), raw.body)
        .expect("interpret should succeed");

    assert_eq!(persona.name, Some("coder".to_string()));
    assert_eq!(persona.model, Some("openai/gpt-4o".to_string()));
    assert!(
        (persona.temperature.expect("temperature should be Some") - 0.2).abs() < 1e-9,
        "temperature should be approximately 0.2"
    );
    assert_eq!(persona.max_tokens, Some(8192));
    assert_eq!(persona.max_tool_turns, Some(20));
    assert_eq!(persona.max_tool_calls_per_subturn, Some(3));
    assert_eq!(persona.max_tool_result_bytes, Some(50000));
    assert_eq!(persona.additional_params, None);
    assert!(
        persona.body.contains("focused coding agent"),
        "body should contain 'focused coding agent'"
    );
}

#[test]
fn interpret_all_fields_together() {
    let mut mapping = noyalib::Mapping::new();
    mapping.insert("name", noyalib::Value::String("full-agent".to_string()));
    mapping.insert(
        "description",
        noyalib::Value::String("A complete agent".to_string()),
    );
    mapping.insert("model", noyalib::Value::String("claude-3".to_string()));
    let mut perms = noyalib::Mapping::new();
    perms.insert("*", noyalib::Value::String("allow".to_string()));
    mapping.insert("permissions", noyalib::Value::Mapping(perms.clone()));

    let result = interpret_front_matter(Some(&mapping), "body content".to_string());
    assert!(result.is_ok());
    let persona = result.unwrap();
    assert_eq!(persona.name, Some("full-agent".to_string()));
    assert_eq!(persona.description, Some("A complete agent".to_string()));
    assert_eq!(persona.model, Some("claude-3".to_string()));
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
    assert_eq!(persona.permissions, None);
    assert_eq!(persona.body, "body");
}

#[test]
fn list_available_returns_empty_when_no_dirs() {
    let temp_cwd = TempDir::new().unwrap();
    let temp_config = TempDir::new().unwrap();
    let resolver = FsPersonaResolver::new(
        temp_cwd.path().to_path_buf(),
        temp_config.path().to_path_buf(),
        no_builtins(),
    );
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
    )
    .unwrap();

    let resolver = FsPersonaResolver::new(
        temp_cwd.path().to_path_buf(),
        temp_config.path().to_path_buf(),
        no_builtins(),
    );
    let result = resolver.list_available();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].name, "coder");
    assert_eq!(result[0].description.as_deref(), Some("Writes code"));
    assert!(!result[0].builtin);
}

#[test]
fn list_available_deduplicates_cwd_over_xdg() {
    let temp_cwd = TempDir::new().unwrap();
    let temp_config = TempDir::new().unwrap();

    let cwd_agents = temp_cwd.path().join(".agents");
    fs::create_dir_all(&cwd_agents).unwrap();
    fs::write(
        cwd_agents.join("coder.md"),
        "---\nname: local-coder\ndescription: Local\n---\n",
    )
    .unwrap();

    let xdg_agents = temp_config.path().join("agents");
    fs::create_dir_all(&xdg_agents).unwrap();
    fs::write(
        xdg_agents.join("coder.md"),
        "---\nname: global-coder\ndescription: Global\n---\n",
    )
    .unwrap();

    let resolver = FsPersonaResolver::new(
        temp_cwd.path().to_path_buf(),
        temp_config.path().to_path_buf(),
        no_builtins(),
    );
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

    let resolver = FsPersonaResolver::new(
        temp_cwd.path().to_path_buf(),
        temp_config.path().to_path_buf(),
        no_builtins(),
    );
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
    fs::write(
        agents_dir.join("my-agent.md"),
        "# Just a body, no front matter",
    )
    .unwrap();

    let resolver = FsPersonaResolver::new(
        temp_cwd.path().to_path_buf(),
        temp_config.path().to_path_buf(),
        no_builtins(),
    );
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

    let resolver = FsPersonaResolver::new(
        temp_cwd.path().to_path_buf(),
        temp_config.path().to_path_buf(),
        no_builtins(),
    );
    let result = resolver.list_available();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].name, "coder");
}

// Built-in persona resolution tests

#[test]
fn resolve_builtin_planner_enabled() {
    let temp_cwd = TempDir::new().unwrap();
    let temp_config = TempDir::new().unwrap();

    let resolver = FsPersonaResolver::new(
        temp_cwd.path().to_path_buf(),
        temp_config.path().to_path_buf(),
        AgentsConfig::default(),
    );
    let result = resolver.resolve("planner");

    assert!(result.is_ok());
    let (path, content) = result.unwrap();
    assert!(path.to_str().unwrap().contains("<builtin>"));
    assert_eq!(content, BUILTIN_PLANNER_CONTENT);
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
        temp_config.path().to_path_buf(),
        config,
    );
    let result = resolver.resolve("planner");

    assert!(result.is_err());
    assert!(matches!(result, Err(PersonaError::NotFound { .. })));
}

#[test]
fn resolve_builtin_falls_through_when_disabled() {
    let temp_cwd = TempDir::new().unwrap();
    let temp_config = TempDir::new().unwrap();

    // Create a filesystem planner.md so fallback succeeds
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
        temp_config.path().to_path_buf(),
        config,
    );
    let result = resolver.resolve("planner");

    assert!(result.is_ok());
    let (path, content) = result.unwrap();
    assert_eq!(path, fs_path);
    assert_eq!(content, "# Custom planner");
}

#[test]
fn list_available_includes_enabled_builtins() {
    let temp_cwd = TempDir::new().unwrap();
    let temp_config = TempDir::new().unwrap();

    let resolver = FsPersonaResolver::new(
        temp_cwd.path().to_path_buf(),
        temp_config.path().to_path_buf(),
        AgentsConfig::default(),
    );
    let result = resolver.list_available();

    // Built-ins should be the first entries
    assert!(result.len() >= 2);
    assert_eq!(result[0].name, "planner");
    assert!(result[0].builtin);
    assert_eq!(result[1].name, "maker");
    assert!(result[1].builtin);
}

#[test]
fn list_available_excludes_disabled_builtins() {
    let temp_cwd = TempDir::new().unwrap();
    let temp_config = TempDir::new().unwrap();

    let config = AgentsConfig {
        planner_enabled: false,
        ..AgentsConfig::default()
    };
    let resolver = FsPersonaResolver::new(
        temp_cwd.path().to_path_buf(),
        temp_config.path().to_path_buf(),
        config,
    );
    let result = resolver.list_available();

    // "planner" should not appear as a built-in
    let planner_builtins: Vec<_> = result
        .iter()
        .filter(|s| s.name == "planner" && s.builtin)
        .collect();
    assert!(planner_builtins.is_empty());

    // "maker" should still be present
    assert!(result.iter().any(|s| s.name == "maker" && s.builtin));
}

#[test]
fn resolve_filesystem_persona_still_works() {
    let temp_cwd = TempDir::new().unwrap();
    let temp_config = TempDir::new().unwrap();

    let agents_dir = temp_cwd.path().join(".agents");
    fs::create_dir_all(&agents_dir).unwrap();
    let persona_file = agents_dir.join("custom.md");
    fs::write(&persona_file, "# Custom persona").unwrap();

    let resolver = FsPersonaResolver::new(
        temp_cwd.path().to_path_buf(),
        temp_config.path().to_path_buf(),
        AgentsConfig::default(),
    );
    let result = resolver.resolve("custom");

    assert!(result.is_ok());
    let (path, content) = result.unwrap();
    assert_eq!(path, persona_file);
    assert_eq!(content, "# Custom persona");
}

#[test]
fn list_available_deduplicates_builtin_over_filesystem() {
    let temp_cwd = TempDir::new().unwrap();
    let temp_config = TempDir::new().unwrap();

    // Create a filesystem planner.md that collides with the built-in
    let agents_dir = temp_cwd.path().join(".agents");
    fs::create_dir_all(&agents_dir).unwrap();
    fs::write(
        agents_dir.join("planner.md"),
        "---\nname: planner\ndescription: Custom planner\n---\n",
    )
    .unwrap();

    let resolver = FsPersonaResolver::new(
        temp_cwd.path().to_path_buf(),
        temp_config.path().to_path_buf(),
        AgentsConfig::default(),
    );
    let result = resolver.list_available();

    // "planner" should appear exactly once, as a built-in
    let planner_entries: Vec<_> = result.iter().filter(|s| s.name == "planner").collect();
    assert_eq!(planner_entries.len(), 1);
    assert!(planner_entries[0].builtin);
}

#[test]
fn icon_parsed_from_front_matter() {
    let mut mapping = noyalib::Mapping::new();
    mapping.insert("icon", noyalib::Value::String("🧠".to_string()));
    let result = interpret_front_matter(Some(&mapping), "body".to_string());
    assert!(result.is_ok());
    let persona = result.unwrap();
    assert_eq!(persona.icon.as_deref(), Some("🧠"));
}

#[test]
fn no_icon_defaults_to_none() {
    let mapping = noyalib::Mapping::new();
    let result = interpret_front_matter(Some(&mapping), "body".to_string());
    assert!(result.is_ok());
    let persona = result.unwrap();
    assert_eq!(persona.icon, None);
}

#[test]
fn icon_must_be_string() {
    let mut mapping = noyalib::Mapping::new();
    mapping.insert("icon", noyalib::Value::Number(noyalib::Number::from(42)));
    let result = interpret_front_matter(Some(&mapping), "body".to_string());
    assert!(result.is_err());
}
