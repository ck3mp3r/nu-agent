use super::*;

/// `write_file` replaces a file and `read_file` reads it back.
#[test]
fn write_and_read_file() -> Result<()> {
    // -- Setup & Fixtures
    let store = memory_store("session-files")?;

    // -- Exec & Check: a new file is written
    store.write_file("notes/a.txt", b"one")?;
    assert_eq!(store.read_file("notes/a.txt")?, b"one");

    // -- Exec & Check: writing again replaces it
    store.write_file("notes/a.txt", b"two")?;
    assert_eq!(store.read_file("notes/a.txt")?, b"two");
    Ok(())
}

/// `append_file` adds to a file, creating it when it is not there.
#[test]
fn append_file_creates_and_appends() -> Result<()> {
    // -- Setup & Fixtures
    let store = memory_store("session-append-file")?;

    // -- Exec
    store.append_file("log.txt", b"one")?;
    store.append_file("log.txt", b"two")?;

    // -- Check
    assert_eq!(store.read_file("log.txt")?, b"onetwo");
    Ok(())
}

/// `files` lists the names of the session's files.
#[test]
fn files_lists_names() -> Result<()> {
    // -- Setup & Fixtures
    let store = memory_store("session-list-files")?;
    store.write_file("b.txt", b"b")?;
    store.write_file("a.txt", b"a")?;

    // -- Exec
    let names = store.files()?;

    // -- Check
    assert_eq!(names, vec!["a.txt".to_owned(), "b.txt".to_owned()]);
    Ok(())
}

/// `remove_file` removes a file, and removing one that is not there succeeds.
#[test]
fn remove_file_is_idempotent() -> Result<()> {
    // -- Setup & Fixtures
    let store = memory_store("session-remove-file")?;
    store.write_file("a.txt", b"a")?;

    // -- Exec
    store.remove_file("a.txt")?;
    store.remove_file("a.txt")?;

    // -- Check
    assert!(store.files()?.is_empty());
    Ok(())
}

/// `read_file` of a file that is not there is `NotFound`.
#[test]
fn read_file_missing_is_not_found() -> Result<()> {
    // -- Setup & Fixtures
    let store = memory_store("session-file-not-found")?;

    // -- Exec
    let error = store
        .read_file("a.txt")
        .err()
        .ok_or("a missing file is an error")?;

    // -- Check
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    Ok(())
}

/// A name that leaves the store is refused.
#[test]
fn names_outside_the_store_are_refused() -> Result<()> {
    // -- Setup & Fixtures
    let store = memory_store("session-names")?;

    // -- Exec & Check
    assert!(store.write_file("../elsewhere", b"x").is_err());
    assert!(store.read_log("a/b").is_err());
    Ok(())
}
