//! Unit tests for the rig-memory store seam.

use super::{RigStoreConfig, StoreType};
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

/// A store path opens exactly that file: no app-dir join, no derived tree.
#[tokio::test]
async fn store_path_opens_exactly_that_file() -> Result<()> {
    // -- Setup & Fixtures
    let temp = TempDir::new()?;
    let store_path = temp.path().join("pinned").join("my-store.db");
    std::fs::create_dir_all(store_path.parent().ok_or("the path has a parent")?)?;
    let rig = RigStoreConfig::new(StoreType::Sqlite).with_store_path(&store_path);

    // -- Exec
    let store = rig.open("session-pinned").await?;
    store.append_log("agent", 0, b"one")?;

    // -- Check
    assert!(store_path.is_file(), "the store opens the pinned file");
    assert_eq!(rig.open("session-pinned").await?.read_log("agent")?, b"one");
    assert!(
        !temp.path().join(APP_DIR_NAME).exists(),
        "no derived tree is created beside the pinned file"
    );
    Ok(())
}

/// `NU_AGENT_CACHE_HOME` re-roots the store under `APP_DIR_NAME`.
#[tokio::test]
async fn cache_home_env_reroots_the_store_under_the_app_dir() -> Result<()> {
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
    let expected = cache_home.join(APP_DIR_NAME).join("sessions.db");

    // -- Exec
    let rig = RigStoreConfig::new(StoreType::Sqlite).with_store_path(&paths.store_path);
    rig.open("session-env")
        .await?
        .append_log("agent", 0, b"one")?;

    // -- Check
    assert_eq!(paths.store_path, expected);
    assert!(expected.is_file(), "the re-rooted file holds the store");
    Ok(())
}

/// A `XDG_CACHE_HOME` override re-roots the derived store path under
/// `APP_DIR_NAME`.
#[tokio::test]
async fn xdg_cache_home_reroots_the_store_under_the_app_dir() -> Result<()> {
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
    let expected = cache_home.join(APP_DIR_NAME).join("sessions.db");

    // -- Exec
    let rig = RigStoreConfig::new(StoreType::Sqlite).with_store_path(&paths.store_path);
    rig.open("session-xdg")
        .await?
        .append_log("agent", 0, b"one")?;

    // -- Check
    assert_eq!(paths.store_path, expected);
    assert!(expected.is_file(), "the store keeps APP_DIR_NAME");
    Ok(())
}

/// One config opens one file: the manager and the runtime share the store.
#[tokio::test]
async fn one_config_opens_one_file_for_every_site() -> Result<()> {
    // -- Setup & Fixtures
    let temp = TempDir::new()?;
    let store_path = temp.path().join("shared.db");
    let rig = RigStoreConfig::new(StoreType::Sqlite).with_store_path(&store_path);

    // -- Exec: the manager writes through its clone of the config
    rig.clone()
        .open("session-shared")
        .await?
        .append_log("agent", 0, b"one")?;

    // -- Check: the runtime reads the same file through its own clone
    assert_eq!(
        rig.clone()
            .open("session-shared")
            .await?
            .read_log("agent")?,
        b"one"
    );
    Ok(())
}

/// A memory store opens and lists with no ambient cache location at all: with
/// `HOME` and `XDG_CACHE_HOME` unset the factory resolves no path for it.
#[tokio::test]
#[serial_test::serial]
async fn in_memory_opens_without_home_or_xdg_cache_home() -> Result<()> {
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
        .await
        .map_err(|e| format!("the memory store must open with no ambient path: {e:?}"));
    let listed = rig
        .session_ids()
        .await
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
