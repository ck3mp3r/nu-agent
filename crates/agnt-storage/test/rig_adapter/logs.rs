use super::*;

/// `append_log` starts a log at length 0 and appends to it at its length.
#[test]
fn test_rig_adapter_append_log_starts_and_appends() -> Result<()> {
    // -- Setup & Fixtures
    let store = memory_store("session-logs")?;

    // -- Exec & Check: the first append starts the log
    store.append_log("agent", 0, b"one")?;
    assert_eq!(store.read_log("agent")?, b"one");
    assert_eq!(store.logs()?, vec!["agent".to_owned()]);

    // -- Exec & Check: the next append goes after it
    store.append_log("agent", 3, b"two")?;
    assert_eq!(store.read_log("agent")?, b"onetwo");
    Ok(())
}

/// `append_log` refuses a stale length and writes nothing.
#[test]
fn test_rig_adapter_append_log_refuses_stale_length() -> Result<()> {
    // -- Setup & Fixtures
    let (db, store) = memory_database("session-stale")?;
    store.append_log("agent", 0, b"abc")?;

    // -- Exec
    let refused = store.append_log("agent", 2, b"x");

    // -- Check
    assert!(
        matches!(refused, Err(LogWriteError::Changed)),
        "a stale length is refused: {refused:?}"
    );
    assert_eq!(store.read_log("agent")?, b"abc", "nothing is written");
    assert_eq!(session_ids(&db)?, vec!["session-stale".to_owned()]);
    Ok(())
}

/// `append_log` refuses a log that is not there when the length is not 0, and
/// records no session: nothing persisted.
#[test]
fn test_rig_adapter_append_log_refuses_missing_log() -> Result<()> {
    // -- Setup & Fixtures
    let (db, store) = memory_database("session-missing")?;

    // -- Exec
    let refused = store.append_log("agent", 5, b"x");

    // -- Check
    assert!(matches!(refused, Err(LogWriteError::Changed)));
    assert!(store.logs()?.is_empty(), "no log is started");
    assert!(session_ids(&db)?.is_empty(), "no session is recorded");
    Ok(())
}

/// The length a log write names counts bytes, not characters.
#[test]
fn test_rig_adapter_append_log_counts_bytes() -> Result<()> {
    // -- Setup & Fixtures: "é" is two bytes in UTF-8
    let store = memory_store("session-bytes")?;
    store.append_log("agent", 0, "é".as_bytes())?;

    // -- Exec & Check: the log is two bytes long, so a write naming 2 appends
    store.append_log("agent", 2, "é".as_bytes())?;
    assert_eq!(store.read_log("agent")?, "éé".as_bytes());
    Ok(())
}

/// A log holds bytes that are not text: an append keeps them whole.
#[test]
fn test_rig_adapter_append_log_keeps_binary_bytes() -> Result<()> {
    // -- Setup & Fixtures: a NUL, a lone continuation byte, and an invalid pair
    let store = memory_store("session-binary")?;
    let first = [0x00u8, 0xff, 0x80, 0x0a];
    let second = [0xc3u8, 0x28, 0x00];
    let mut expected = first.to_vec();
    expected.extend_from_slice(&second);

    // -- Exec
    store.append_log("agent", 0, &first)?;
    store.append_log("agent", 4, &second)?;

    // -- Check
    assert_eq!(store.read_log("agent")?, expected);
    Ok(())
}

/// `replace_log` replaces the whole log under the same length condition.
#[test]
fn test_rig_adapter_replace_log_replaces_and_refuses_stale() -> Result<()> {
    // -- Setup & Fixtures
    let store = memory_store("session-replace")?;
    store.append_log("agent", 0, b"abc")?;

    // -- Exec & Check: a write naming the log's length replaces it
    store.replace_log("agent", 3, b"xy")?;
    assert_eq!(store.read_log("agent")?, b"xy");

    // -- Exec & Check: a stale length is refused
    let refused = store.replace_log("agent", 3, b"z");
    assert!(matches!(refused, Err(LogWriteError::Changed)));
    assert_eq!(store.read_log("agent")?, b"xy");
    Ok(())
}

/// `replace_log` starts a log that is not there.
#[test]
fn test_rig_adapter_replace_log_starts_missing_log() -> Result<()> {
    // -- Setup & Fixtures
    let (db, store) = memory_database("session-replace-start")?;

    // -- Exec
    store.replace_log("agent", 0, b"new")?;

    // -- Check
    assert_eq!(store.read_log("agent")?, b"new");
    assert_eq!(session_ids(&db)?, vec!["session-replace-start".to_owned()]);
    Ok(())
}

/// `read_log` of a log that is not there is `NotFound`.
#[test]
fn test_rig_adapter_read_log_missing_is_not_found() -> Result<()> {
    // -- Setup & Fixtures
    let store = memory_store("session-not-found")?;

    // -- Exec
    let error = store
        .read_log("agent")
        .err()
        .ok_or("a missing log is an error")?;

    // -- Check
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    Ok(())
}

/// `destroy` removes the session's logs, its files, and its session row.
#[test]
fn test_rig_adapter_destroy_removes_everything() -> Result<()> {
    // -- Setup & Fixtures
    let (db, store) = memory_database("session-destroy")?;
    store.append_log("agent", 0, b"log")?;
    store.write_file("name", b"a name")?;

    // -- Exec
    store.destroy()?;

    // -- Check
    assert!(store.logs()?.is_empty());
    assert!(store.files()?.is_empty());
    assert!(
        session_ids(&db)?.is_empty(),
        "the destroyed session is listed no more"
    );
    Ok(())
}

/// Two sessions in one database do not see each other's logs.
#[test]
fn test_rig_adapter_logs_are_scoped_to_the_session() -> Result<()> {
    // -- Setup & Fixtures
    let temp = TempDir::new()?;
    let path = temp.path().join("sessions.db");
    let first = file_store(&path, "first")?;
    let second = file_store(&path, "second")?;

    // -- Exec
    first.append_log("agent", 0, b"one")?;

    // -- Check
    assert_eq!(first.logs()?, vec!["agent".to_owned()]);
    assert!(second.logs()?.is_empty(), "the other session has no logs");
    Ok(())
}
