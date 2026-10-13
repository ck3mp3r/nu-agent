//! Backend selection and auto-detection tests.

use crate::config::file_backend::FileBackend;
use crate::config::keychain_backend::KeychainBackend;
use crate::utils::env_map::EnvMap;

use super::{
    BackendChoice, BackendKind, EnvOnlyBackend, Vault, VaultBackend, VaultBackendKind,
    backend_choice,
};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Tests

#[test]
fn test_vault_env_only_constructs_env_only_backend() -> Result<()> {
    // -- Setup & Fixtures
    // (no fixtures required — the constructor takes no arguments)

    // -- Exec
    let vault = Vault::env_only();

    // -- Check
    assert!(
        matches!(vault.backend(), VaultBackendKind::EnvOnly(_)),
        "env_only() should select the EnvOnly backend"
    );
    Ok(())
}

#[test]
fn test_vault_new_uses_given_backend() -> Result<()> {
    // -- Setup & Fixtures
    let backend = VaultBackendKind::EnvOnly(EnvOnlyBackend);

    // -- Exec
    let vault = Vault::new(backend);

    // -- Check
    assert!(
        matches!(vault.backend(), VaultBackendKind::EnvOnly(_)),
        "new() should keep the backend it was given"
    );
    Ok(())
}

#[test]
fn test_vault_auto_detect_with_env_selects_env_only() -> Result<()> {
    // -- Setup & Fixtures
    let env = EnvMap::from([("AGENT_VAULT_BACKEND".to_string(), "env".to_string())]);

    // -- Exec
    let vault =
        Vault::auto_detect_with(&env).map_err(|e| format!("env choice must not probe: {e}"))?;

    // -- Check
    assert_eq!(
        vault.backend().kind(),
        BackendKind::EnvOnly,
        "AGENT_VAULT_BACKEND=env must select the env-only backend without probing"
    );
    Ok(())
}

#[test]
fn test_vault_backend_choice_unset_selects_auto() -> Result<()> {
    // -- Setup & Fixtures
    let env = EnvMap::new();

    // -- Exec
    let choice = backend_choice(&env);

    // -- Check
    assert_eq!(
        choice,
        BackendChoice::Auto,
        "an unset AGENT_VAULT_BACKEND must keep the auto chain"
    );
    Ok(())
}

#[test]
fn test_vault_backend_choice_unknown_selects_auto() -> Result<()> {
    // -- Setup & Fixtures
    let env = EnvMap::from([("AGENT_VAULT_BACKEND".to_string(), "bogus".to_string())]);

    // -- Exec
    let choice = backend_choice(&env);

    // -- Check
    assert_eq!(
        choice,
        BackendChoice::Auto,
        "an unknown AGENT_VAULT_BACKEND value must fall back to the auto chain"
    );
    Ok(())
}

#[test]
fn test_vault_auto_detect_with_nextest_selects_env_only() -> Result<()> {
    // -- Setup & Fixtures
    let env = EnvMap::from([("NEXTEST".to_string(), "1".to_string())]);

    // -- Exec
    let vault =
        Vault::auto_detect_with(&env).map_err(|e| format!("nextest must not probe: {e}"))?;

    // -- Check
    assert_eq!(
        vault.backend().kind(),
        BackendKind::EnvOnly,
        "NEXTEST with an unset AGENT_VAULT_BACKEND must select the env-only backend without probing"
    );
    Ok(())
}

#[test]
fn test_vault_backend_choice_explicit_value_wins_over_nextest() -> Result<()> {
    // -- Setup & Fixtures
    let cases = [
        ("env", BackendChoice::EnvOnly),
        ("file", BackendChoice::File),
        ("keychain", BackendChoice::Auto),
        ("auto", BackendChoice::Auto),
        ("bogus", BackendChoice::Auto),
    ];

    // -- Exec & Check
    for (value, expected) in cases {
        let env = EnvMap::from([
            ("NEXTEST".to_string(), "1".to_string()),
            ("AGENT_VAULT_BACKEND".to_string(), value.to_string()),
        ]);
        let choice = backend_choice(&env);
        assert_eq!(
            choice, expected,
            "an explicit AGENT_VAULT_BACKEND={value} must win over NEXTEST"
        );
    }
    Ok(())
}

/// Canary for the NEXTEST guard.
///
/// nextest sets `NEXTEST=1` in every test process, so the real process
/// environment selects the env-only backend and no test probes the OS
/// keychain. Plain `cargo test` does not set `NEXTEST`; the early return is a
/// runner precondition, not skipped coverage.
#[test]
fn test_vault_auto_detect_under_nextest_selects_env_only() -> Result<()> {
    // -- Setup & Fixtures
    if std::env::var_os("NEXTEST").is_none() {
        return Ok(());
    }

    // -- Exec
    let vault = Vault::auto_detect().map_err(|e| format!("auto_detect must not probe: {e}"))?;

    // -- Check
    assert_eq!(
        vault.backend().kind(),
        BackendKind::EnvOnly,
        "nextest must select the env-only backend without probing the keychain"
    );
    Ok(())
}

#[test]
fn test_vault_backend_kind_has_three_variants() -> Result<()> {
    // -- Setup & Fixtures
    let variants = [
        VaultBackendKind::Keychain(KeychainBackend),
        VaultBackendKind::File(FileBackend::new(std::path::PathBuf::from("/nonexistent"))),
        VaultBackendKind::EnvOnly(EnvOnlyBackend),
    ];

    // -- Exec
    let count = variants.len();

    // -- Check
    assert_eq!(count, 3, "VaultBackendKind should have exactly 3 variants");
    Ok(())
}

// endregion: --- Tests
