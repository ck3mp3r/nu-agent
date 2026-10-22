use super::*;

/// The seam opens a SQLite store at `{cache}/{APP_DIR_NAME}/rig-sessions.db`.
#[test]
fn seam_sqlite_uses_the_session_database() -> Result<()> {
    // -- Setup & Fixtures
    let temp = TempDir::new()?;

    // -- Exec
    let store = create_rig_store_in(StoreType::Sqlite, temp.path(), "session-seam")?;
    store.append_log("agent", 0, b"one")?;

    // -- Check
    let path = crate::utils::paths::app_dir(temp.path()).join("rig-sessions.db");
    assert!(
        path.is_file(),
        "the database is in the app directory: {path:?}"
    );
    assert_eq!(store.read_log("agent")?, b"one");
    Ok(())
}

/// The seam opens an in-memory store that writes nothing to disk.
#[test]
fn seam_memory_writes_nothing() -> Result<()> {
    // -- Setup & Fixtures
    let temp = TempDir::new()?;

    // -- Exec
    let store = create_rig_store_in(StoreType::Memory, temp.path(), "session-memory")?;
    store.append_log("agent", 0, b"one")?;

    // -- Check
    assert_eq!(store.read_log("agent")?, b"one");
    assert_eq!(
        std::fs::read_dir(temp.path())?.count(),
        0,
        "the in-memory store writes nothing"
    );
    Ok(())
}
