use super::*;

/// `session_ids` lists every session of the database in id order.
#[test]
fn session_ids_lists_the_sessions_in_order() -> Result<()> {
    // -- Setup & Fixtures: two sessions, written out of id order
    let temp = TempDir::new()?;
    let path = temp.path().join("sessions.db");
    let path = path.to_str().ok_or("the path is UTF-8")?;
    RigSqliteStore::open(path, "zeta")?.append_log("agent", 0, b"one")?;
    RigSqliteStore::open(path, "alpha")?.append_log("agent", 0, b"two")?;

    // -- Exec
    let ids = RigSqliteStore::session_ids(path)?;

    // -- Check
    assert_eq!(ids, vec!["alpha".to_owned(), "zeta".to_owned()]);
    Ok(())
}
