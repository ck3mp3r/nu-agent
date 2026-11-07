//! VaultError Display tests.

use super::VaultError;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Tests

#[test]
fn test_vault_error_keychain_display_contains_message() -> Result<()> {
    // -- Setup & Fixtures
    let error = VaultError::Keychain("test".to_string());

    // -- Exec
    let rendered = error.to_string();

    // -- Check
    assert!(
        rendered.contains("Keychain error: test"),
        "Display should contain the keychain message, got: {rendered}"
    );
    Ok(())
}

#[test]
fn test_vault_error_key_not_found_display_contains_key_name() -> Result<()> {
    // -- Setup & Fixtures
    let error = VaultError::KeyNotFound("mcp:bearer:missing".to_string());

    // -- Exec
    let rendered = error.to_string();

    // -- Check
    assert!(
        rendered.contains("mcp:bearer:missing"),
        "Display should contain the missing key name, got: {rendered}"
    );
    Ok(())
}

#[test]
fn test_vault_error_remaining_variants_display() -> Result<()> {
    // -- Setup & Fixtures
    let errors = [
        (
            VaultError::Io(std::io::Error::other("boom")),
            "I/O error: boom",
        ),
        (VaultError::NoBackend, "No vault backend available"),
        (
            VaultError::NoDataDir,
            "No data directory found — set XDG_DATA_HOME",
        ),
    ];

    // -- Exec & Check
    for (error, expected) in errors {
        let rendered = error.to_string();
        assert_eq!(rendered, expected, "unexpected Display output");
    }
    Ok(())
}

// endregion: --- Tests
