//! Integration tests for `dispatch::utility::run_session_command`.
//!
//! Every test seeds a real SQLite store inside the tempdir XDG cache and then
//! drives the `session` subcommand against the process working directory.

use super::*;

use agnt_core::session::prefix::{dir_prefix, dir_prefix_legacy};
use agnt_core::session::{SessionStore, StoreType, create_store};
use agnt_core::types::{Message, Text, UserContent};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Test Support

/// Seed one session whose id carries `prefix`, and return the bare id.
async fn seed_session(prefix: &str, bare_id: &str) -> Result<()> {
    let store = create_store(StoreType::Sqlite).await?;
    let messages = vec![Message::User {
        content: vec![UserContent::Text(Text::new("seeded prompt"))],
    }];
    store
        .create(&format!("{prefix}-{bare_id}"), &messages)
        .await?;
    Ok(())
}

/// The session id prefix derived from the process working directory.
fn cwd_prefix() -> Result<String> {
    Ok(dir_prefix(&std::env::current_dir()?))
}

// endregion: --- Test Support

// region:    --- Tests

#[tokio::test]
#[serial_test::serial]
async fn test_integration_session_list_without_sessions_reports_none() -> Result<()> {
    // -- Setup & Fixtures
    let _env = support::TestEnv::new().await?;
    let (args, command) = support::parse_session_command(&["session", "list"])?;

    // -- Exec
    dispatch::run_session_command(&args, &command).await?;

    // -- Check
    // The command reports "No sessions found" and returns Ok.
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn test_integration_session_list_renders_seeded_session() -> Result<()> {
    // -- Setup & Fixtures
    let env = support::TestEnv::new().await?;
    seed_session(&cwd_prefix()?, "listcase").await?;
    let (args, command) = support::parse_session_command(&["session", "list"])?;

    // -- Exec
    dispatch::run_session_command(&args, &command).await?;

    // -- Check
    assert!(
        env.cache_home_path("nu-agent/sessions.db").exists(),
        "the sqlite store must live under the tempdir cache home"
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn test_integration_session_inspect_renders_seeded_session() -> Result<()> {
    // -- Setup & Fixtures
    let _env = support::TestEnv::new().await?;
    seed_session(&cwd_prefix()?, "inspectcase").await?;
    let (args, command) = support::parse_session_command(&["session", "inspect", "inspectcase"])?;

    // -- Exec
    dispatch::run_session_command(&args, &command).await?;

    // -- Check
    // The command prints the session metadata and the seeded message.
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn test_integration_session_inspect_missing_session_errors() -> Result<()> {
    // -- Setup & Fixtures
    let _env = support::TestEnv::new().await?;
    let (args, command) = support::parse_session_command(&["session", "inspect", "absent"])?;

    // -- Exec
    let err = dispatch::run_session_command(&args, &command)
        .await
        .err()
        .ok_or("an unknown session id must fail")?;

    // -- Check
    assert!(
        err.to_string().contains("Session not found: absent"),
        "got: {err}"
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn test_integration_session_clear_deletes_seeded_session() -> Result<()> {
    // -- Setup & Fixtures
    let _env = support::TestEnv::new().await?;
    let prefix = cwd_prefix()?;
    seed_session(&prefix, "clearcase").await?;
    let (args, command) = support::parse_session_command(&["session", "clear", "clearcase"])?;

    // -- Exec
    dispatch::run_session_command(&args, &command).await?;

    // -- Check
    let store = create_store(StoreType::Sqlite).await?;
    let loaded = store.load(&format!("{prefix}-clearcase")).await?;
    assert!(loaded.is_none(), "the cleared session must be gone");
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn test_integration_session_clear_falls_back_to_legacy_prefix() -> Result<()> {
    // -- Setup & Fixtures
    let _env = support::TestEnv::new().await?;
    let cwd = std::env::current_dir()?;
    let legacy_prefix = dir_prefix_legacy(&cwd);
    seed_session(&legacy_prefix, "legacycase").await?;
    let (args, command) = support::parse_session_command(&["session", "clear", "legacycase"])?;

    // -- Exec
    dispatch::run_session_command(&args, &command).await?;

    // -- Check
    let store = create_store(StoreType::Sqlite).await?;
    let loaded = store.load(&format!("{legacy_prefix}-legacycase")).await?;
    assert!(loaded.is_none(), "the legacy session must be gone");
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn test_integration_session_clear_missing_session_errors() -> Result<()> {
    // -- Setup & Fixtures
    let _env = support::TestEnv::new().await?;
    let (args, command) = support::parse_session_command(&["session", "clear", "absent"])?;

    // -- Exec
    let err = dispatch::run_session_command(&args, &command)
        .await
        .err()
        .ok_or("an unknown session id must fail")?;

    // -- Check
    assert!(
        err.to_string().contains("Session not found: absent"),
        "got: {err}"
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn test_integration_session_list_uses_jsonl_store() -> Result<()> {
    // -- Setup & Fixtures
    let _env = support::TestEnv::new().await?;
    let (args, command) = support::parse_session_command(&["session", "list", "--store", "jsonl"])?;

    // -- Exec
    dispatch::run_session_command(&args, &command).await?;

    // -- Check
    // The jsonl backend resolves its base path from the tempdir cache home.
    Ok(())
}

// endregion: --- Tests
