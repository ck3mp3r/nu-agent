//! Tests for `verify_copilot_login_persistence` and `read_copilot_token_record`.

use super::{read_copilot_token_record, verify_copilot_login_persistence};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Tests

#[test]
fn test_copilot_tokens_verify_ok_for_non_empty_token() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::TempDir::new()?;
    std::fs::write(
        dir.path().join("api-key.json"),
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
    std::fs::write(dir.path().join("api-key.json"), "not json")?;

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
        dir.path().join("api-key.json"),
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
    std::fs::write(dir.path().join("api-key.json"), r#"{"expires_at": 1}"#)?;

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
        dir.path().join("api-key.json"),
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
    std::fs::write(dir.path().join("api-key.json"), r#"{"expires_at": 1}"#)?;

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
    std::fs::write(dir.path().join("api-key.json"), "not json")?;

    // -- Exec
    let result = read_copilot_token_record(dir.path());

    // -- Check
    assert!(result.is_err(), "an unparsable file must error");
    Ok(())
}

// endregion: --- Tests
