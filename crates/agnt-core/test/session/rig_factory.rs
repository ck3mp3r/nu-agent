//! Unit tests for the rig-memory store seam.

use super::{
    RigStoreConfig, RigStoreError, StoreType, create_rig_store_at, create_rig_store_in,
    session_dir, sessions_root,
};
use crate::config::Paths;
use crate::test_support::env_map;
use crate::utils::paths::APP_DIR_NAME;
use rig_memory::Store;
use tempfile::TempDir;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// The default store type is SQLite.
#[test]
fn store_type_default_is_sqlite() {
    assert_eq!(StoreType::default(), StoreType::Sqlite);
}

/// The seam opens a `DirStore` for `jsonl` and keeps the session in its own
/// directory.
#[test]
fn seam_jsonl_uses_the_session_directory() -> Result<()> {
    // -- Setup & Fixtures
    let temp = TempDir::new()?;

    // -- Exec
    let store = create_rig_store_in(StoreType::Jsonl, temp.path(), "session-jsonl")?;
    store.append_log("agent", 0, b"one")?;

    // -- Check
    let dir = session_dir(temp.path(), "session-jsonl");
    assert!(
        dir.join("logs").join("agent.jsonl").is_file(),
        "the log is in the session directory"
    );
    assert_eq!(store.read_log("agent")?, b"one");
    Ok(())
}

/// The seam refuses a session the JSONL store nu-agent kept before rig-memory
/// saved.
#[test]
fn seam_refuses_legacy_jsonl_session() -> Result<()> {
    // -- Setup & Fixtures
    let temp = TempDir::new()?;
    let sessions = crate::utils::paths::app_dir(temp.path()).join("sessions");
    std::fs::create_dir_all(&sessions)?;
    std::fs::write(sessions.join("session-legacy.jsonl"), b"{}\n")?;

    // -- Exec
    let refused = create_rig_store_in(StoreType::Jsonl, temp.path(), "session-legacy");

    // -- Check
    let error = refused.err().ok_or("a legacy session is refused")?;
    assert!(
        matches!(&error, RigStoreError::Legacy),
        "the error names the legacy layout: {error}"
    );
    assert!(
        error.to_string().contains("start a new session"),
        "the error says what to do: {error}"
    );
    Ok(())
}

/// A store path opens exactly that file: no app-dir join, no derived tree.
#[test]
fn store_path_opens_exactly_that_file() -> Result<()> {
    // -- Setup & Fixtures
    let temp = TempDir::new()?;
    let store_path = temp.path().join("pinned").join("my-store.db");
    std::fs::create_dir_all(store_path.parent().ok_or("the path has a parent")?)?;
    let rig = RigStoreConfig::new(StoreType::Sqlite).with_store_path(&store_path);

    // -- Exec
    let store = rig.open("session-pinned")?;
    store.append_log("agent", 0, b"one")?;

    // -- Check
    assert!(store_path.is_file(), "the store opens the pinned file");
    assert_eq!(rig.open("session-pinned")?.read_log("agent")?, b"one");
    assert!(
        !temp.path().join(APP_DIR_NAME).exists(),
        "no derived tree is created beside the pinned file"
    );
    Ok(())
}

/// `NU_AGENT_CACHE_HOME` re-roots the store under `APP_DIR_NAME`.
#[test]
fn cache_home_env_reroots_the_store_under_the_app_dir() -> Result<()> {
    // -- Setup & Fixtures
    let temp = TempDir::new()?;
    let cache_home = temp.path().join("elsewhere");
    let env = env_map(&[
        (
            "NU_AGENT_CACHE_HOME",
            cache_home.to_str().ok_or("utf8 path")?,
        ),
        ("XDG_CONFIG_HOME", "/xdg/config"),
        ("XDG_DATA_HOME", "/xdg/data"),
        ("XDG_STATE_HOME", "/xdg/state"),
    ]);
    let paths = Paths::resolve_with(None, temp.path(), &env).map_err(|e| e.to_string())?;
    let expected = cache_home.join(APP_DIR_NAME).join("rig-sessions.db");

    // -- Exec
    let rig = RigStoreConfig::new(StoreType::Sqlite).with_store_path(&paths.store_path);
    rig.open("session-env")?.append_log("agent", 0, b"one")?;

    // -- Check
    assert_eq!(paths.store_path, expected);
    assert!(expected.is_file(), "the re-rooted file holds the store");
    Ok(())
}

/// A `XDG_CACHE_HOME` override re-roots the derived store path under
/// `APP_DIR_NAME`.
#[test]
fn xdg_cache_home_reroots_the_store_under_the_app_dir() -> Result<()> {
    // -- Setup & Fixtures
    let temp = TempDir::new()?;
    let cache_home = temp.path().join("xdg-cache");
    let env = env_map(&[
        ("XDG_CACHE_HOME", cache_home.to_str().ok_or("utf8 path")?),
        ("XDG_CONFIG_HOME", "/xdg/config"),
        ("XDG_DATA_HOME", "/xdg/data"),
        ("XDG_STATE_HOME", "/xdg/state"),
    ]);
    let paths = Paths::resolve_with(None, temp.path(), &env).map_err(|e| e.to_string())?;
    let expected = cache_home.join(APP_DIR_NAME).join("rig-sessions.db");

    // -- Exec
    let rig = RigStoreConfig::new(StoreType::Sqlite).with_store_path(&paths.store_path);
    rig.open("session-xdg")?.append_log("agent", 0, b"one")?;

    // -- Check
    assert_eq!(paths.store_path, expected);
    assert!(expected.is_file(), "the store keeps APP_DIR_NAME");
    Ok(())
}

/// The JSONL backend keeps its session directories beside a pinned store path.
#[test]
fn store_path_keeps_jsonl_sessions_beside_it() -> Result<()> {
    // -- Setup & Fixtures
    let temp = TempDir::new()?;
    let store_dir = temp.path().join(APP_DIR_NAME);
    let store_path = store_dir.join("rig-sessions.db");

    // -- Exec
    let store = create_rig_store_at(StoreType::Jsonl, &store_path, "session-jsonl-path")?;
    store.append_log("agent", 0, b"one")?;

    // -- Check
    let dir = sessions_root(&store_dir).join("session-jsonl-path");
    assert!(
        dir.join("logs").join("agent.jsonl").is_file(),
        "the session directory sits beside the store path"
    );
    assert_eq!(store.read_log("agent")?, b"one");
    Ok(())
}

/// A pinned JSONL store path lists the sessions stored beside it.
///
/// The listing reads `sessions_root(store_dir(store_path))` — the same root
/// [`RigStoreConfig::open`] writes to — so a pinned path lists what it holds.
#[test]
fn pinned_jsonl_store_path_lists_the_sessions_beside_it() -> Result<()> {
    // -- Setup & Fixtures
    let temp = TempDir::new()?;
    let store_path = temp.path().join(APP_DIR_NAME).join("rig-sessions.db");
    let store = create_rig_store_at(StoreType::Jsonl, &store_path, "session-listed")?;
    store.append_log("agent", 0, b"one")?;

    // -- Exec
    let ids = RigStoreConfig::new(StoreType::Jsonl)
        .with_store_path(&store_path)
        .session_ids()?;

    // -- Check: the listing reads beside the pinned path, not the derived tree
    assert_eq!(ids, vec!["session-listed".to_string()]);
    Ok(())
}

/// One config opens one file: the manager and the runtime share the store.
#[test]
fn one_config_opens_one_file_for_every_site() -> Result<()> {
    // -- Setup & Fixtures
    let temp = TempDir::new()?;
    let store_path = temp.path().join("shared.db");
    let rig = RigStoreConfig::new(StoreType::Sqlite).with_store_path(&store_path);

    // -- Exec: the manager writes through its clone of the config
    rig.clone()
        .open("session-shared")?
        .append_log("agent", 0, b"one")?;

    // -- Check: the runtime reads the same file through its own clone
    assert_eq!(
        rig.clone().open("session-shared")?.read_log("agent")?,
        b"one"
    );
    Ok(())
}

/// A memory store opens and lists with no ambient cache location at all: with
/// `HOME` and `XDG_CACHE_HOME` unset the factory resolves no path for it.
#[test]
#[serial_test::serial]
fn in_memory_opens_without_home_or_xdg_cache_home() -> Result<()> {
    // -- Setup & Fixtures: a process with no ambient cache location.
    let previous_home = std::env::var_os("HOME");
    let previous_xdg_cache_home = std::env::var_os("XDG_CACHE_HOME");
    unsafe {
        std::env::remove_var("HOME");
        std::env::remove_var("XDG_CACHE_HOME");
    }
    let rig = RigStoreConfig::in_memory();

    // -- Exec: open and list before the environment is restored.
    let opened = rig
        .open("session-memory")
        .map_err(|e| format!("the memory store must open with no ambient path: {e:?}"));
    let listed = rig
        .session_ids()
        .map_err(|e| format!("the memory store must list with no ambient path: {e:?}"));

    // -- Setup & Fixtures: restore the environment whatever the result.
    unsafe {
        match previous_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
        match previous_xdg_cache_home {
            Some(value) => std::env::set_var("XDG_CACHE_HOME", value),
            None => std::env::remove_var("XDG_CACHE_HOME"),
        }
    }

    // -- Check: the store opens, keeps nothing, and lists no session.
    let store = opened?;
    store.append_log("agent", 0, b"one")?;
    assert_eq!(store.read_log("agent")?, b"one");
    assert!(listed?.is_empty(), "a memory store lists no session");
    Ok(())
}
