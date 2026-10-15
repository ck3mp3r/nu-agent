//! Integration tests for `dispatch::utility::run_config_command`.
//!
//! `config init` resolves its target path from the XDG config home, so the
//! tests drive it against the tempdir tree.

use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Tests

#[tokio::test]
#[serial_test::serial]
async fn test_integration_config_init_creates_then_refuses_without_force() -> Result<()> {
    // -- Setup & Fixtures
    let env = support::TestEnv::new().await?;
    let command = support::parse_config_command(&["config", "init"])?;

    // -- Exec
    dispatch::run_config_command(&command)?;

    // -- Check
    assert!(
        env.config_path().exists(),
        "config init must write config.toml"
    );

    // -- Exec
    let err = dispatch::run_config_command(&command)
        .err()
        .ok_or("a second config init must refuse to overwrite")?;

    // -- Check
    assert!(err.to_string().contains("already exists"), "got: {err}");

    // -- Exec
    let forced = support::parse_config_command(&["config", "init", "--force"])?;
    dispatch::run_config_command(&forced)?;

    // -- Check
    assert!(
        env.config_path().exists(),
        "--force must rewrite config.toml"
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn test_integration_config_init_uses_env_provider_and_model() -> Result<()> {
    // -- Setup & Fixtures
    let env = support::TestEnv::new().await?;
    let _vars = support::set_env(&[("AGENT_PROVIDER", "ollama"), ("AGENT_MODEL", "llama3")]);
    let command = support::parse_config_command(&["config", "init"])?;

    // -- Exec
    dispatch::run_config_command(&command)?;

    // -- Check
    let written = std::fs::read_to_string(env.config_path())?;
    assert!(
        written.contains("model = \"ollama/llama3\""),
        "the env provider and model must seed the active model block"
    );
    Ok(())
}

// endregion: --- Tests
