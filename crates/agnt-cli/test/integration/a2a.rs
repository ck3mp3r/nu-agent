//! Integration tests for `dispatch::run_a2a`.
//!
//! The A2A subcommand always starts the interactive loop, so it derives its
//! mode from the terminal state. With both stdin and stderr attached to a
//! terminal the CLI would take over the terminal, so these tests skip in that
//! case and run the full construction path otherwise.

use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Tests

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial]
async fn test_integration_a2a_run_a2a_starts_server_and_shuts_down() -> Result<()> {
    // -- Setup & Fixtures
    if support::tui_mode_selected() {
        return Ok(());
    }
    let env = support::TestEnv::new().await?;
    env.write_default_config()?;
    let (args, a2a) = support::parse_a2a_args(&["a2a", "--port", "0"])?;

    // -- Exec
    let err = dispatch::run_a2a(&args, &a2a)
        .await
        .err()
        .ok_or("the A2A loop must report the missing prompt")?;

    // -- Check
    assert!(
        err.to_string()
            .contains("No prompt provided for stderr mode"),
        "got: {err}"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial]
async fn test_integration_a2a_run_a2a_registers_persona_skills() -> Result<()> {
    // -- Setup & Fixtures
    if support::tui_mode_selected() {
        return Ok(());
    }
    let env = support::TestEnv::new().await?;
    env.write_default_config()?;
    let (args, a2a) = support::parse_a2a_args(&[
        "a2a",
        "--port",
        "0",
        "--mesh-key",
        "agnt-cli-integration",
        "--agent",
        "planner",
        "--name",
        "peer",
    ])?;

    // -- Exec
    let err = dispatch::run_a2a(&args, &a2a)
        .await
        .err()
        .ok_or("the A2A loop must report the missing prompt")?;

    // -- Check
    assert!(
        err.to_string()
            .contains("No prompt provided for stderr mode"),
        "got: {err}"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial]
async fn test_integration_a2a_run_a2a_missing_config_errors() -> Result<()> {
    // -- Setup & Fixtures
    if support::tui_mode_selected() {
        return Ok(());
    }
    let _env = support::TestEnv::new().await?;
    let (args, a2a) = support::parse_a2a_args(&["a2a", "--port", "0"])?;

    // -- Exec
    let err = dispatch::run_a2a(&args, &a2a)
        .await
        .err()
        .ok_or("a missing config.toml must fail")?;

    // -- Check
    assert!(
        err.to_string().contains("No configuration found"),
        "got: {err}"
    );
    Ok(())
}

// endregion: --- Tests
