use std::ffi::OsString;
use std::path::Path;

use serial_test::serial;
use tempfile::TempDir;

use super::{load_env_file, load_env_files, parse_line};

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
#[serial]
fn test_dotenv_load_env_files_reads_user_level_file() -> Result<()> {
    // -- Setup & Fixtures
    let key = "NU_AGENT_TEST_USER_LEVEL";
    with_env_scope(&[key], |dir| {
        let env_path = dir.path().join("nu-agent").join(".env");
        write_env_file(&env_path, &format!("{key}=sk-from-file\n"))?;

        // -- Exec
        load_env_files();

        // -- Check
        assert_eq!(
            std::env::var(key).as_deref(),
            Ok("sk-from-file"),
            "user-level .env value is loaded"
        );
        Ok(())
    })
}

#[test]
#[serial]
fn test_dotenv_existing_env_var_takes_precedence() -> Result<()> {
    // -- Setup & Fixtures
    let key = "NU_AGENT_TEST_PRECEDENCE";
    with_env_scope(&[key], |dir| {
        let env_path = dir.path().join("nu-agent").join(".env");
        write_env_file(&env_path, &format!("{key}=sk-from-file\n"))?;
        unsafe {
            std::env::set_var(key, "sk-from-shell");
        }

        // -- Exec
        load_env_files();

        // -- Check
        assert_eq!(
            std::env::var(key).as_deref(),
            Ok("sk-from-shell"),
            "an existing env var is never overwritten"
        );
        Ok(())
    })
}

#[test]
#[serial]
fn test_dotenv_load_env_files_is_silent_when_files_absent() -> Result<()> {
    // -- Setup & Fixtures
    let key = "NU_AGENT_TEST_ABSENT";
    with_env_scope(&[key], |dir| {
        // No nu-agent/.env is written into the temp config dir.
        let missing = dir.path().join("nu-agent").join(".env");
        assert!(!missing.exists(), "fixture file must not exist");

        // -- Exec
        load_env_files();

        // -- Check
        assert_eq!(
            std::env::var(key).ok(),
            None,
            "a missing file leaves the env untouched"
        );
        Ok(())
    })
}

#[test]
#[serial]
fn test_dotenv_load_env_file_skips_malformed_but_loads_valid_lines() -> Result<()> {
    // -- Setup & Fixtures
    let good_key = "NU_AGENT_TEST_GOOD_LINE";
    let bad_key = "NU_AGENT_TEST_BAD_LINE";
    with_env_scope(&[good_key, bad_key], |dir| {
        let env_path = dir.path().join("malformed.env");
        let body = format!("this line has no equals sign\n{good_key}=valid\nanother bad line\n");
        write_env_file(&env_path, &body)?;

        // -- Exec
        load_env_file(&env_path);

        // -- Check
        assert_eq!(
            std::env::var(good_key).as_deref(),
            Ok("valid"),
            "valid lines after a malformed line are still loaded"
        );
        assert_eq!(
            std::env::var(bad_key).ok(),
            None,
            "the malformed line is skipped"
        );
        Ok(())
    })
}

#[test]
#[serial]
fn test_dotenv_load_env_file_reads_all_supported_line_forms() -> Result<()> {
    // -- Setup & Fixtures
    let comment_key = "NU_AGENT_TEST_COMMENT";
    let export_key = "NU_AGENT_TEST_EXPORT";
    let double_key = "NU_AGENT_TEST_DOUBLE";
    let single_key = "NU_AGENT_TEST_SINGLE";
    with_env_scope(&[comment_key, export_key, double_key, single_key], |dir| {
        let env_path = dir.path().join("forms.env");
        let body = format!(
            "# {comment_key}=should-not-be-set\nexport {export_key}=exp\n{double_key}=\"dq\"\n{single_key}='sq'\n"
        );
        write_env_file(&env_path, &body)?;

        // -- Exec
        load_env_file(&env_path);

        // -- Check
        assert_eq!(
            std::env::var(comment_key).ok(),
            None,
            "a comment line is skipped"
        );
        assert_eq!(std::env::var(export_key).as_deref(), Ok("exp"));
        assert_eq!(std::env::var(double_key).as_deref(), Ok("dq"));
        assert_eq!(std::env::var(single_key).as_deref(), Ok("sq"));
        Ok(())
    })
}

#[test]
#[serial]
fn test_dotenv_load_env_file_missing_path_is_silent() -> Result<()> {
    // -- Setup & Fixtures
    let dir = TempDir::new()?;
    let missing = dir.path().join("does-not-exist.env");

    // -- Exec
    load_env_file(&missing);

    // -- Check
    assert!(!missing.exists(), "the missing path stays missing");
    Ok(())
}

// endregion: --- Tests

// region:    --- Test Support

/// Run `test` with `XDG_CONFIG_HOME` pointed at a temp dir and with `keys`
/// cleared. Both the config dir and the keys are restored afterwards.
fn with_env_scope<F>(keys: &[&str], test: F) -> Result<()>
where
    F: FnOnce(&TempDir) -> Result<()>,
{
    let dir = TempDir::new()?;
    let previous_xdg = std::env::var_os("XDG_CONFIG_HOME");
    let previous_keys: Vec<(&str, Option<OsString>)> = keys
        .iter()
        .map(|key| (*key, std::env::var_os(key)))
        .collect();

    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", dir.path());
        for key in keys {
            std::env::remove_var(key);
        }
    }

    let outcome = test(&dir);

    unsafe {
        match previous_xdg {
            Some(value) => std::env::set_var("XDG_CONFIG_HOME", value),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
        for (key, value) in previous_keys {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }

    outcome
}

/// Write `body` to `path`, creating parent directories.
fn write_env_file(path: &Path, body: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, body)?;
    Ok(())
}

// endregion: --- Test Support
