//! Tests for `verify_copilot_login_persistence` and `read_copilot_token_record`.

use super::{copilot_token_dir, read_copilot_token_record, verify_copilot_login_persistence};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// The Copilot directory name the current build must resolve.
#[cfg(debug_assertions)]
const EXPECTED_COPILOT_DIR: &str = "github_copilot-dev";

/// The Copilot directory name the current build must resolve.
#[cfg(not(debug_assertions))]
const EXPECTED_COPILOT_DIR: &str = "github_copilot";

// region:    --- Test Support

/// Points `XDG_CONFIG_HOME` at a tempdir for the lifetime of the value.
struct ConfigHomeGuard {
    previous: Option<std::ffi::OsString>,
}

impl ConfigHomeGuard {
    fn apply(dir: &std::path::Path) -> Self {
        let previous = std::env::var_os("XDG_CONFIG_HOME");
        // SAFETY: `set_var` is unsafe because a concurrent reader of the
        // environment is undefined behavior. Under nextest every test runs in
        // its own process, so this mutation is isolated by construction. Under
        // plain `cargo test` the tests share one process and `#[serial]` only
        // orders this test against other `#[serial]` tests — it does not stop
        // unrelated parallel tests from reading the environment. The mutation
        // is therefore harmless in practice, not safe by guarantee.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", dir) };
        Self { previous }
    }
}

impl Drop for ConfigHomeGuard {
    fn drop(&mut self) {
        match self.previous.take() {
            // SAFETY: see `ConfigHomeGuard::apply`.
            Some(previous) => unsafe { std::env::set_var("XDG_CONFIG_HOME", previous) },
            // SAFETY: see `ConfigHomeGuard::apply`.
            None => unsafe { std::env::remove_var("XDG_CONFIG_HOME") },
        }
    }
}

// endregion: --- Test Support

// region:    --- Tests

#[cfg(not(target_os = "windows"))]
#[test]
#[serial_test::serial]
fn test_copilot_tokens_token_dir_flips_name_on_debug_assertions() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::TempDir::new()?;
    let guard = ConfigHomeGuard::apply(dir.path());

    // -- Exec
    let resolved = copilot_token_dir();

    // -- Check
    let resolved = resolved.ok_or("XDG_CONFIG_HOME is set, so a dir must resolve")?;
    assert_eq!(resolved, dir.path().join(EXPECTED_COPILOT_DIR));
    drop(guard);
    Ok(())
}

#[test]
fn test_copilot_tokens_verify_ok_for_non_empty_token() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::TempDir::new()?;
    std::fs::write(
        dir.path().join("copilot.json"),
        r#"{"token": "copilot-key", "expires_at": 4102444800}"#,
    )?;

    // -- Exec
    let result = verify_copilot_login_persistence(dir.path());

    // -- Check
    assert!(result.is_ok(), "a non-empty token verifies: {result:?}");
    Ok(())
}

#[test]
fn test_copilot_tokens_verify_err_when_file_missing() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::TempDir::new()?;

    // -- Exec
    let result = verify_copilot_login_persistence(dir.path());

    // -- Check
    assert!(result.is_err(), "a missing file must not verify");
    Ok(())
}

#[test]
fn test_copilot_tokens_verify_err_when_unparsable() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::TempDir::new()?;
    std::fs::write(dir.path().join("copilot.json"), "not json")?;

    // -- Exec
    let result = verify_copilot_login_persistence(dir.path());

    // -- Check
    assert!(result.is_err(), "an unparsable file must not verify");
    Ok(())
}

#[test]
fn test_copilot_tokens_verify_err_when_token_empty() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::TempDir::new()?;
    std::fs::write(
        dir.path().join("copilot.json"),
        r#"{"token": "   ", "expires_at": 4102444800}"#,
    )?;

    // -- Exec
    let result = verify_copilot_login_persistence(dir.path());

    // -- Check
    assert!(result.is_err(), "a blank token must not verify");
    Ok(())
}

#[test]
fn test_copilot_tokens_verify_err_when_token_absent() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::TempDir::new()?;
    std::fs::write(dir.path().join("copilot.json"), r#"{"expires_at": 1}"#)?;

    // -- Exec
    let result = verify_copilot_login_persistence(dir.path());

    // -- Check
    assert!(result.is_err(), "a record without a token must not verify");
    Ok(())
}

#[test]
fn test_copilot_tokens_read_ok_with_token_and_expiry() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::TempDir::new()?;
    std::fs::write(
        dir.path().join("copilot.json"),
        r#"{"token": "copilot-key", "expires_at": 4102444800}"#,
    )?;

    // -- Exec
    let record = read_copilot_token_record(dir.path())?;

    // -- Check
    let record = record.ok_or("a present file should read as Some")?;
    assert!(record.has_token, "the token is non-empty");
    assert_eq!(record.expires_at, Some(4102444800));
    Ok(())
}

#[test]
fn test_copilot_tokens_read_ok_without_token() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::TempDir::new()?;
    std::fs::write(dir.path().join("copilot.json"), r#"{"expires_at": 1}"#)?;

    // -- Exec
    let record = read_copilot_token_record(dir.path())?;

    // -- Check
    let record = record.ok_or("a present file should read as Some")?;
    assert!(!record.has_token, "an absent token reads as no token");
    assert_eq!(record.expires_at, Some(1));
    Ok(())
}

#[test]
fn test_copilot_tokens_read_none_when_file_missing() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::TempDir::new()?;

    // -- Exec
    let record = read_copilot_token_record(dir.path())?;

    // -- Check
    assert!(record.is_none(), "a missing file reads as None");
    Ok(())
}

#[test]
fn test_copilot_tokens_read_err_when_unparsable() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::TempDir::new()?;
    std::fs::write(dir.path().join("copilot.json"), "not json")?;

    // -- Exec
    let result = read_copilot_token_record(dir.path());

    // -- Check
    assert!(result.is_err(), "an unparsable file must error");
    Ok(())
}

// endregion: --- Tests
