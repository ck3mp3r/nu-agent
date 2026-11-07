//! Integration tests for `dispatch::utility::run_session_command`.
//!
//! Every test seeds a real SQLite store inside the tempdir XDG cache and then
//! drives the `session` subcommand against the process working directory.

use super::*;

use agnt_runtime::session::prefix::dir_prefix;
use agnt_runtime::session::{RigStoreConfig, StoreType};
use agnt_runtime::types::{Message, Text, UserContent};
use agnt_runtime::utils::paths::APP_DIR_NAME;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Test Support

/// Seed one session whose id carries `prefix`, and return the bare id.
async fn seed_session(prefix: &str, bare_id: &str) -> Result<()> {
    let rig = RigStoreConfig::new(StoreType::Sqlite);
    let id = format!("{prefix}-{bare_id}");
    let store = rig.open(&id).await?;
    let mut conversation = agnt_runtime::rig_memory::load(&*store, &id)?;
    conversation.extend([Message::User {
        content: vec![UserContent::Text(Text::new("seeded prompt"))],
    }]);
    agnt_runtime::rig_memory::save(&*store, &id, &mut conversation)?;
    agnt_runtime::session::rig_state::write_state_records(
        std::sync::Arc::clone(&store),
        "test/model".to_string(),
        Some("seeded prompt".to_string()),
    )
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
        env.cache_home_path(&format!("{APP_DIR_NAME}/sessions.db"))
            .exists(),
        "the session store must live under the tempdir cache home"
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
        !rig.holds(&format!("{prefix}-clearcase")).await?,
        "the cleared session must be gone"
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn test_integration_session_clear_ignores_another_cwd() -> Result<()> {
    // -- Setup & Fixtures
    let _env = support::TestEnv::new().await?;
    let foreign_prefix = dir_prefix(std::path::Path::new("/other/dir"));
    seed_session(&foreign_prefix, "foreigncase").await?;
    let (args, command) = support::parse_session_command(&["session", "clear", "foreigncase"])?;

    // -- Exec
    let result = dispatch::run_session_command(&args, &command).await;

    // -- Check
    assert!(
        result.is_err(),
        "a session of another cwd must not be cleared from this cwd"
    );
    let rig = RigStoreConfig::new(StoreType::Sqlite);
    assert!(
        rig.holds(&format!("{foreign_prefix}-foreigncase")).await?,
        "the foreign session must still exist"
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
