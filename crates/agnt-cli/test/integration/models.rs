//! Integration tests for `dispatch::utility::run_models_command`.
//!
//! The models cache is read from `$XDG_DATA_HOME/{APP_DIR_NAME}/models.json`, so
//! the tests write a fixture into the tempdir data home. `models sync` reaches
//! models.dev over the network and is out of scope.

use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Tests

#[tokio::test]
#[serial_test::serial]
async fn test_integration_models_list_renders_cached_models() -> Result<()> {
    // -- Setup & Fixtures
    let env = support::TestEnv::new().await?;
    support::write_models_cache(&env)?;
    let command = support::parse_models_command(&["models", "list"])?;

    // -- Exec
    dispatch::run_models_command(&command)?;

    // -- Check
    // The command prints one table row per cached model.
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn test_integration_models_list_filters_by_provider() -> Result<()> {
    // -- Setup & Fixtures
    let env = support::TestEnv::new().await?;
    support::write_models_cache(&env)?;
    let matching = support::parse_models_command(&["models", "list", "--provider", "ollama"])?;
    let absent = support::parse_models_command(&["models", "list", "--provider", "absent"])?;

    // -- Exec
    dispatch::run_models_command(&matching)?;
    dispatch::run_models_command(&absent)?;

    // -- Check
    // The matching filter prints rows; the absent filter reports "No models found.".
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn test_integration_models_list_without_cache_errors() -> Result<()> {
    // -- Setup & Fixtures
    let _env = support::TestEnv::new().await?;
    let command = support::parse_models_command(&["models", "list"])?;

    // -- Exec
    let err = dispatch::run_models_command(&command)
        .err()
        .ok_or("a missing models cache must fail")?;

    // -- Check
    assert!(
        err.to_string().contains("Failed to load models cache"),
        "got: {err}"
    );
    Ok(())
}

// endregion: --- Tests
