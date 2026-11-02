use super::*;

/// Answers the names of the tables the database holds, in order.
struct Tables;

impl crate::Work for Tables {
    type Output = sqlx::Result<Vec<String>>;

    async fn run(self, pool: sqlx::SqlitePool) -> Self::Output {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .fetch_all(&pool)
        .await?;
        Ok(rows.into_iter().map(|(name,)| name).collect())
    }
}

/// The session listing holds every session of the database in id order.
#[test]
fn test_rig_adapter_session_ids_lists_the_sessions_in_order() -> Result<()> {
    // -- Setup & Fixtures: two sessions, persisted out of id order
    let temp = TempDir::new()?;
    let path = temp.path().join("sessions.db");
    file_store(&path, "zeta")?.append_log("agent", 0, b"one")?;
    file_store(&path, "alpha")?.append_log("agent", 0, b"two")?;

    // -- Exec
    let ids = session_ids(&Storage::open(&path)?)?;

    // -- Check
    assert_eq!(ids, vec!["alpha".to_owned(), "zeta".to_owned()]);
    Ok(())
}

/// A file write records the session, so the listing needs no log fold.
#[test]
fn test_rig_adapter_file_write_records_the_session() -> Result<()> {
    // -- Setup & Fixtures
    let temp = TempDir::new()?;
    let path = temp.path().join("sessions.db");
    file_store(&path, "session-file-first")?.write_file("name", b"a name")?;

    // -- Exec
    let ids = session_ids(&Storage::open(&path)?)?;

    // -- Check
    assert_eq!(ids, vec!["session-file-first".to_owned()]);
    Ok(())
}

/// A fresh file database is fully migrated and lists no sessions: the picker's
/// empty state before the first write.
#[test]
fn test_rig_adapter_fresh_file_lists_no_sessions() -> Result<()> {
    // -- Setup & Fixtures
    let temp = TempDir::new()?;
    let path = temp.path().join("sessions.db");
    let storage = Storage::open(&path)?;

    // -- Exec
    let ids = session_ids(&storage)?;

    // -- Check: nothing listed, and the six engine tables are there.
    assert!(ids.is_empty(), "a fresh store lists no session: {ids:?}");
    let tables = storage.call_blocking(Tables)??;
    assert_eq!(
        tables,
        [
            "entry_turns",
            "files",
            "logs",
            "sessions",
            "tool_calls",
            "turns"
        ],
        "a fresh store SHALL hold the six engine tables"
    );
    Ok(())
}

/// Repeated writes keep one session row, not a row per write.
#[test]
fn test_rig_adapter_repeated_writes_keep_one_session_row() -> Result<()> {
    // -- Setup & Fixtures
    let (db, store) = memory_database("session-once")?;

    // -- Exec
    store.append_log("agent", 0, b"one")?;
    store.append_log("agent", 3, b"two")?;
    store.write_file("name", b"a name")?;

    // -- Check
    assert_eq!(session_ids(&db)?, vec!["session-once".to_owned()]);
    Ok(())
}

/// A file database keeps its sessions across engines, and creates the nested
/// directory the path names.
#[test]
fn test_rig_adapter_file_database_survives_its_engine() -> Result<()> {
    // -- Setup & Fixtures
    let temp = TempDir::new()?;
    let path = temp.path().join("nested").join("sessions.db");

    // -- Exec: write, drop the engine, read back through a new one
    {
        let store = file_store(&path, "session-reopen")?;
        store.append_log("agent", 0, b"one")?;
    }
    let reopened = file_store(&path, "session-reopen")?;

    // -- Check
    assert!(path.is_file(), "the database is in the nested directory");
    assert_eq!(reopened.read_log("agent")?, b"one");
    assert_eq!(session_ids(&Storage::open(&path)?)?, ["session-reopen"]);
    Ok(())
}
