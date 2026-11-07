use std::sync::{Arc, Mutex};

use crate::config::file_backend::FileBackend;
use crate::config::keychain_backend::KeychainBackend;

use super::{BackendChoice, VaultBackendKind, VaultError, auto_chain_with, resolve_backend_with};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Tests

#[test]
fn test_vault_probe_auto_chain_keychain_ok_skips_file_probe() -> Result<()> {
    // -- Setup & Fixtures
    let calls = Arc::new(Mutex::new(Vec::<&'static str>::new()));
    let keychain_calls = Arc::clone(&calls);
    let file_calls = Arc::clone(&calls);

    // -- Exec
    let backend = auto_chain_with(
        move || {
            keychain_calls.lock().unwrap().push("keychain");
            Ok(KeychainBackend)
        },
        move || {
            file_calls.lock().unwrap().push("file");
            Ok(FileBackend::new("/nonexistent"))
        },
    )?;

    // -- Check
    assert!(
        matches!(backend, VaultBackendKind::Keychain(_)),
        "an available keychain must win the chain"
    );
    let recorded = calls.lock().unwrap().clone();
    assert_eq!(
        recorded,
        vec!["keychain"],
        "the file probe must not run once the keychain probe succeeds"
    );
    Ok(())
}

#[test]
fn test_vault_probe_auto_chain_no_backend_then_file_ok() -> Result<()> {
    // -- Setup & Fixtures
    // (probes are the fixtures — no real keychain or filesystem is touched)

    // -- Exec
    let backend = auto_chain_with(
        || Err(VaultError::NoBackend),
        || Ok(FileBackend::new("/nonexistent")),
    )?;

    // -- Check
    assert!(
        matches!(backend, VaultBackendKind::File(_)),
        "a missing keychain must fall through to the file backend"
    );
    Ok(())
}

#[test]
fn test_vault_probe_auto_chain_no_backend_then_file_err_falls_back_to_env_only() -> Result<()> {
    // -- Setup & Fixtures
    // (probes are the fixtures — no real keychain or filesystem is touched)

    // -- Exec
    let backend = auto_chain_with(|| Err(VaultError::NoBackend), || Err(VaultError::NoDataDir))?;

    // -- Check
    assert!(
        matches!(backend, VaultBackendKind::EnvOnly(_)),
        "a missing keychain and an unavailable file backend must fall back to env-only"
    );
    Ok(())
}

#[test]
fn test_vault_probe_auto_chain_keychain_locked_is_backend_unavailable() -> Result<()> {
    // -- Setup & Fixtures
    // (probes are the fixtures — no real keychain or filesystem is touched)

    // -- Exec
    let result = auto_chain_with(
        || Err(VaultError::Keychain("locked".to_string())),
        || Ok(FileBackend::new("/nonexistent")),
    );

    // -- Check
    let error = result
        .err()
        .ok_or("a locked keychain must abort the chain")?;
    assert!(
        matches!(&error, VaultError::BackendUnavailable(msg) if msg.contains("Keychain error: locked")),
        "the keychain error must be carried into BackendUnavailable, got: {error:?}"
    );
    Ok(())
}

#[test]
fn test_vault_probe_resolve_backend_file_choice_ok_and_err() -> Result<()> {
    // -- Setup & Fixtures
    // (probes are the fixtures — no real keychain or filesystem is touched)

    // -- Exec
    let available = resolve_backend_with(
        BackendChoice::File,
        || Err(VaultError::NoBackend),
        || Ok(FileBackend::new("/nonexistent")),
    )?;
    let unavailable = resolve_backend_with(
        BackendChoice::File,
        || Err(VaultError::NoBackend),
        || Err(VaultError::NoDataDir),
    )?;

    // -- Check
    assert!(
        matches!(available, VaultBackendKind::File(_)),
        "an Ok file probe must select the file backend"
    );
    assert!(
        matches!(unavailable, VaultBackendKind::EnvOnly(_)),
        "an Err file probe must fall back to env-only"
    );
    Ok(())
}

#[test]
fn test_vault_probe_resolve_backend_env_only_choice_skips_probes() -> Result<()> {
    // -- Setup & Fixtures
    let calls = Arc::new(Mutex::new(Vec::<&'static str>::new()));
    let keychain_calls = Arc::clone(&calls);
    let file_calls = Arc::clone(&calls);

    // -- Exec
    let backend = resolve_backend_with(
        BackendChoice::EnvOnly,
        move || {
            keychain_calls.lock().unwrap().push("keychain");
            Ok(KeychainBackend)
        },
        move || {
            file_calls.lock().unwrap().push("file");
            Ok(FileBackend::new("/nonexistent"))
        },
    )?;

    // -- Check
    assert!(
        matches!(backend, VaultBackendKind::EnvOnly(_)),
        "the env-only choice must select the env-only backend"
    );
    let recorded = calls.lock().unwrap().clone();
    assert!(
        recorded.is_empty(),
        "no probe may run for the env-only choice, got: {recorded:?}"
    );
    Ok(())
}

// endregion: --- Tests
