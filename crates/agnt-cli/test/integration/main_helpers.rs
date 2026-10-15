//! Integration tests for the `main.rs` prompt and logging helpers.
//!
//! `main()` itself cannot be called from a test, but `resolve_prompt`,
//! `init_logging`, and `log_target` are reachable and cover the CLI's startup
//! wiring.

use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Tests

#[test]
fn test_integration_main_resolve_prompt_returns_positional() {
    // -- Exec
    let prompt = resolve_prompt(Some("hello".to_string()));

    // -- Check
    assert_eq!(prompt.as_deref(), Some("hello"));
}

#[test]
fn test_integration_main_resolve_prompt_without_positional_uses_stdin() {
    // -- Setup & Fixtures
    // The test harness inherits stdin. A terminal or an empty pipe yields no
    // prompt; a pipe that carried text yields that text.

    // -- Exec
    let prompt = resolve_prompt(None);

    // -- Check
    if let Some(text) = prompt {
        assert!(
            !text.trim().is_empty(),
            "a piped prompt must be non-empty after trimming"
        );
    }
}

#[tokio::test]
#[serial_test::serial]
async fn test_integration_main_init_logging_targets_state_dir() -> Result<()> {
    // -- Setup & Fixtures
    let env = support::TestEnv::new().await?;
    let cli = support::parse_cli(&["--log-level", "debug", "hello"])?;

    // -- Exec
    init_logging(&cli.args);

    // -- Check
    let log_path = env.state_home_path("nu-agent/logs/agent.log");
    assert!(
        log_path.exists(),
        "init_logging must open the state-dir log file at {}",
        log_path.display()
    );
    Ok(())
}

// endregion: --- Tests
