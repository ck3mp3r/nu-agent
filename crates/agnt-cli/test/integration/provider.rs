//! Integration tests for `dispatch::utility::run_provider_command`.
//!
//! The vault is the file backend rooted in the tempdir data home, so the
//! credential write, list, and delete paths all run without a keychain probe.
//! The GitHub Copilot device-code flow reaches the network and is out of scope.

use super::*;

use agnt_core::utils::paths::APP_DIR_NAME;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Tests

#[tokio::test]
#[serial_test::serial]
async fn test_integration_provider_status_without_credentials_reports_none() -> Result<()> {
    // -- Setup & Fixtures
    let _env = support::TestEnv::new().await?;
    let command = support::parse_provider_command(&["provider", "auth", "status"])?;

    // -- Exec
    dispatch::run_provider_command(&command).await?;

    // -- Check
    // The command reports "No stored provider credentials." and returns Ok.
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn test_integration_provider_login_status_logout_round_trip() -> Result<()> {
    // -- Setup & Fixtures
    let env = support::TestEnv::new().await?;
    let login = support::parse_provider_command(&[
        "provider",
        "auth",
        "login",
        "openai",
        "--api-key",
        "sk-integration",
    ])?;

    // -- Exec
    dispatch::run_provider_command(&login).await?;

    // -- Check
    let secrets = env.data_home_path(&format!("{APP_DIR_NAME}/secrets.json"));
    assert!(
        secrets.exists(),
        "the file vault must hold the stored API key"
    );
    let stored = std::fs::read_to_string(&secrets)?;
    assert!(
        stored.contains("sk-integration"),
        "the stored credential must carry the supplied key"
    );

    // -- Exec
    let status = support::parse_provider_command(&["provider", "auth", "status"])?;
    dispatch::run_provider_command(&status).await?;

    // -- Check
    // The status table lists the stored provider.

    // -- Exec
    let logout = support::parse_provider_command(&["provider", "auth", "logout", "openai"])?;
    dispatch::run_provider_command(&logout).await?;

    // -- Check
    let stored = std::fs::read_to_string(&secrets)?;
    assert!(
        !stored.contains("sk-integration"),
        "logout must remove the stored credential"
    );

    // -- Exec
    dispatch::run_provider_command(&logout).await?;

    // -- Check
    // The second logout reports "No stored credentials for 'openai'".
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn test_integration_provider_logout_copilot_without_token_files() -> Result<()> {
    // -- Setup & Fixtures
    let _env = support::TestEnv::new().await?;
    let command =
        support::parse_provider_command(&["provider", "auth", "logout", "github-copilot"])?;

    // -- Exec
    dispatch::run_provider_command(&command).await?;

    // -- Check
    // With no Copilot token files the command falls through to the vault and
    // reports "No stored credentials".
    Ok(())
}

// endregion: --- Tests
