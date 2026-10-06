use std::path::Path;

use tempfile::TempDir;

use super::{load_env_file_into, load_env_files_into, parse_line};
use crate::utils::env_map::EnvMap;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Tests

#[test]
fn test_dotenv_parse_line_key_value() -> Result<()> {
    // -- Setup & Fixtures
    let line = "OPENAI_API_KEY=sk-from-file";

    // -- Exec
    let parsed = parse_line(line);

    // -- Check
    let (key, value) = parsed
        .map_err(|e| format!("should parse: {e}"))?
        .ok_or("should be an assignment")?;
    assert_eq!(key, "OPENAI_API_KEY");
    assert_eq!(value, "sk-from-file");
    Ok(())
}

#[test]
fn test_dotenv_parse_line_skips_comments_and_blank_lines() -> Result<()> {
    // -- Setup & Fixtures
    let lines = ["# this is a comment", "   ", ""];

    // -- Exec & Check
    for line in lines {
        let parsed = parse_line(line).map_err(|e| format!("should not error: {e}"))?;
        assert!(parsed.is_none(), "should be skipped: {line:?}");
    }
    Ok(())
}

#[test]
fn test_dotenv_parse_line_strips_export_prefix() -> Result<()> {
    // -- Setup & Fixtures
    let line = "export ANTHROPIC_API_KEY=sk-ant";

    // -- Exec
    let parsed = parse_line(line);

    // -- Check
    let (key, value) = parsed
        .map_err(|e| format!("should parse: {e}"))?
        .ok_or("should be an assignment")?;
    assert_eq!(key, "ANTHROPIC_API_KEY");
    assert_eq!(value, "sk-ant");
    Ok(())
}

#[test]
fn test_dotenv_parse_line_strips_double_quotes() -> Result<()> {
    // -- Setup & Fixtures
    let line = "KEY=\"quoted value\"";

    // -- Exec
    let parsed = parse_line(line);

    // -- Check
    let (_, value) = parsed
        .map_err(|e| format!("should parse: {e}"))?
        .ok_or("should be an assignment")?;
    assert_eq!(value, "quoted value");
    Ok(())
}

#[test]
fn test_dotenv_parse_line_strips_single_quotes() -> Result<()> {
    // -- Setup & Fixtures
    let line = "KEY='single quoted'";

    // -- Exec
    let parsed = parse_line(line);

    // -- Check
    let (_, value) = parsed
        .map_err(|e| format!("should parse: {e}"))?
        .ok_or("should be an assignment")?;
    assert_eq!(value, "single quoted");
    Ok(())
}

#[test]
fn test_dotenv_parse_line_keeps_unbalanced_quotes() -> Result<()> {
    // -- Setup & Fixtures
    let line = "KEY=\"unbalanced";

    // -- Exec
    let parsed = parse_line(line);

    // -- Check
    let (_, value) = parsed
        .map_err(|e| format!("should parse: {e}"))?
        .ok_or("should be an assignment")?;
    assert_eq!(value, "\"unbalanced", "only matching pairs are stripped");
    Ok(())
}

#[test]
fn test_dotenv_parse_line_rejects_malformed_line() -> Result<()> {
    // -- Setup & Fixtures
    let line = "this line has no equals sign";

    // -- Exec
    let parsed = parse_line(line);

    // -- Check
    assert!(parsed.is_err(), "a line without '=' is malformed");
    Ok(())
}

#[test]
fn test_dotenv_parse_line_rejects_empty_key() -> Result<()> {
    // -- Setup & Fixtures
    let line = "=value-without-key";

    // -- Exec
    let parsed = parse_line(line);

    // -- Check
    assert!(parsed.is_err(), "an empty key is malformed");
    Ok(())
}

#[test]
fn test_dotenv_parse_line_keeps_equals_signs_in_value() -> Result<()> {
    // -- Setup & Fixtures
    let line = "CONNECTION=host=localhost;port=5432";

    // -- Exec
    let parsed = parse_line(line);

    // -- Check
    let (key, value) = parsed
        .map_err(|e| format!("should parse: {e}"))?
        .ok_or("should be an assignment")?;
    assert_eq!(key, "CONNECTION", "split on the first '=' only");
    assert_eq!(value, "host=localhost;port=5432");
    Ok(())
}

#[test]
fn test_dotenv_load_env_files_reads_user_level_file() -> Result<()> {
    // -- Setup & Fixtures
    let key = "NU_AGENT_TEST_USER_LEVEL";
    let dir = TempDir::new()?;
    let project = TempDir::new()?;
    let env_path = dir.path().join("nu-agent").join(".env");
    write_env_file(&env_path, &format!("{key}=sk-from-file\n"))?;
    let mut env = EnvMap::new();

    // -- Exec
    load_env_files_into(&mut env, Some(dir.path()), project.path());

    // -- Check
    assert_eq!(
        env.get(key).map(String::as_str),
        Some("sk-from-file"),
        "user-level .env value is loaded"
    );
    Ok(())
}

#[test]
fn test_dotenv_existing_env_var_takes_precedence() -> Result<()> {
    // -- Setup & Fixtures
    let key = "NU_AGENT_TEST_PRECEDENCE";
    let dir = TempDir::new()?;
    let project = TempDir::new()?;
    let env_path = dir.path().join("nu-agent").join(".env");
    write_env_file(&env_path, &format!("{key}=sk-from-file\n"))?;
    let mut env = EnvMap::new();
    env.insert(key.to_string(), "sk-from-shell".to_string());

    // -- Exec
    load_env_files_into(&mut env, Some(dir.path()), project.path());

    // -- Check
    assert_eq!(
        env.get(key).map(String::as_str),
        Some("sk-from-shell"),
        "an existing env var is never overwritten"
    );
    Ok(())
}

#[test]
fn test_dotenv_load_env_files_is_silent_when_files_absent() -> Result<()> {
    // -- Setup & Fixtures
    let key = "NU_AGENT_TEST_ABSENT";
    let dir = TempDir::new()?;
    let project = TempDir::new()?;
    // No nu-agent/.env is written into the temp config dir.
    let missing = dir.path().join("nu-agent").join(".env");
    assert!(!missing.exists(), "fixture file must not exist");
    let mut env = EnvMap::new();

    // -- Exec
    load_env_files_into(&mut env, Some(dir.path()), project.path());

    // -- Check
    assert_eq!(
        env.get(key),
        None,
        "a missing file leaves the env untouched"
    );
    Ok(())
}

#[test]
fn test_dotenv_load_env_file_skips_malformed_but_loads_valid_lines() -> Result<()> {
    // -- Setup & Fixtures
    let good_key = "NU_AGENT_TEST_GOOD_LINE";
    let bad_key = "NU_AGENT_TEST_BAD_LINE";
    let dir = TempDir::new()?;
    let env_path = dir.path().join("malformed.env");
    let body = format!("this line has no equals sign\n{good_key}=valid\nanother bad line\n");
    write_env_file(&env_path, &body)?;
    let mut env = EnvMap::new();

    // -- Exec
    load_env_file_into(&mut env, &env_path);

    // -- Check
    assert_eq!(
        env.get(good_key).map(String::as_str),
        Some("valid"),
        "valid lines after a malformed line are still loaded"
    );
    assert_eq!(env.get(bad_key), None, "the malformed line is skipped");
    Ok(())
}

#[test]
fn test_dotenv_load_env_file_reads_all_supported_line_forms() -> Result<()> {
    // -- Setup & Fixtures
    let comment_key = "NU_AGENT_TEST_COMMENT";
    let export_key = "NU_AGENT_TEST_EXPORT";
    let double_key = "NU_AGENT_TEST_DOUBLE";
    let single_key = "NU_AGENT_TEST_SINGLE";
    let dir = TempDir::new()?;
    let env_path = dir.path().join("forms.env");
    let body = format!(
        "# {comment_key}=should-not-be-set\nexport {export_key}=exp\n{double_key}=\"dq\"\n{single_key}='sq'\n"
    );
    write_env_file(&env_path, &body)?;
    let mut env = EnvMap::new();

    // -- Exec
    load_env_file_into(&mut env, &env_path);

    // -- Check
    assert_eq!(env.get(comment_key), None, "a comment line is skipped");
    assert_eq!(env.get(export_key).map(String::as_str), Some("exp"));
    assert_eq!(env.get(double_key).map(String::as_str), Some("dq"));
    assert_eq!(env.get(single_key).map(String::as_str), Some("sq"));
    Ok(())
}

#[test]
fn test_dotenv_load_env_file_missing_path_is_silent() -> Result<()> {
    // -- Setup & Fixtures
    let dir = TempDir::new()?;
    let missing = dir.path().join("does-not-exist.env");
    let mut env = EnvMap::new();

    // -- Exec
    load_env_file_into(&mut env, &missing);

    // -- Check
    assert!(!missing.exists(), "the missing path stays missing");
    assert!(env.is_empty(), "a missing file applies nothing");
    Ok(())
}

// endregion: --- Tests

// region:    --- Test Support

/// Write `body` to `path`, creating parent directories.
fn write_env_file(path: &Path, body: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, body)?;
    Ok(())
}

// endregion: --- Test Support
