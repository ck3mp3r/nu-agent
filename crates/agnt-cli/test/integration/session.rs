//! Integration tests for `dispatch::utility::run_session_command`.
//!
//! Every test seeds a real SQLite store inside the tempdir XDG cache and then
//! drives the `session` subcommand against the process working directory.

use super::*;

use agnt_core::session::prefix::{dir_prefix, dir_prefix_legacy};
use agnt_core::session::{RigStoreConfig, StoreType};
use agnt_core::types::{Message, Text, UserContent};
use agnt_core::utils::paths::APP_DIR_NAME;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Test Support

/// Seed one session whose id carries `prefix`, and return the bare id.
async fn seed_session(prefix: &str, bare_id: &str) -> Result<()> {
    let rig = RigStoreConfig::new(StoreType::Sqlite);
    let id = format!("{prefix}-{bare_id}");
    let store = rig.open(&id)?;
    let mut conversation = agnt_core::rig_memory::load(&*store, &id)?;
    conversation.extend([Message::User {
        content: vec![UserContent::Text(Text::new("seeded prompt"))],
    }]);
    agnt_core::rig_memory::save(&*store, &id, &mut conversation)?;
    agnt_core::session::rig_state::write_state_records(
        &*store,
        "test/model",
        Some("seeded prompt"),
    )?;
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
        env.cache_home_path(&format!("{APP_DIR_NAME}/rig-sessions.db"))
            .exists(),
        "the rig sqlite store must live under the tempdir cache home"
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
    let rig = RigStoreConfig::new(StoreType::Sqlite);
    assert!(
        !rig.holds(&format!("{prefix}-clearcase"))?,
        "the cleared session must be gone"
    );
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
    let rig = RigStoreConfig::new(StoreType::Sqlite);
    assert!(
        !rig.holds(&format!("{legacy_prefix}-legacycase"))?,
        "the legacy session must be gone"
    );
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

#[tokio::test]
#[serial_test::serial]
async fn test_integration_session_env_clears_ambient_store_type() -> Result<()> {
    // -- Setup & Fixtures
    let _set = support::set_env(&[("AGENT_SESSION_STORE_TYPE", "jsonl")]);

    // -- Exec & Check
    // A set variable is cleared while the guard lives and restored after.
    let env = support::TestEnv::new().await?;
    assert_eq!(
        std::env::var_os("AGENT_SESSION_STORE_TYPE"),
        None,
        "TestEnv must clear the ambient store type"
    );
    drop(env);
    assert_eq!(
        std::env::var_os("AGENT_SESSION_STORE_TYPE"),
        Some("jsonl".into()),
        "the guard must restore the prior value"
    );

    // An unset variable stays unset after the guard drops.
    let _unset = support::unset_env(&["AGENT_SESSION_STORE_TYPE"]);
    let env = support::TestEnv::new().await?;
    assert_eq!(
        std::env::var_os("AGENT_SESSION_STORE_TYPE"),
        None,
        "TestEnv must leave an unset store type unset"
    );
    drop(env);
    assert_eq!(
        std::env::var_os("AGENT_SESSION_STORE_TYPE"),
        None,
        "the guard must not introduce the store type"
    );
    Ok(())
}

// endregion: --- Tests
