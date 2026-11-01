use super::MemoryBackendError;
use super::MemoryError;
use super::RigStoreError;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// An `io::Error` wraps as `Backend(Io)`, and renders as the inner error.
#[test]
fn memory_error_backend_io_transparent() -> Result<()> {
    // -- Setup & Fixtures
    let io_error = std::io::Error::new(std::io::ErrorKind::NotFound, "the log is gone");
    let expected = io_error.to_string();

    // -- Exec
    let error = MemoryError::backend(io_error);

    // -- Check
    assert_eq!(
        error.to_string(),
        format!("Memory backend error: {expected}")
    );
    let MemoryError::Backend(MemoryBackendError::Io(inner)) = &error else {
        return Err(format!("an io error must wrap as Backend(Io), got {error:?}").into());
    };
    assert_eq!(inner.to_string(), expected);
    Ok(())
}

/// A rig `SessionError` wraps as `Backend(Rig)`.
#[test]
fn memory_error_backend_rig_transparent() -> Result<()> {
    // -- Setup & Fixtures
    let session_error = rig_memory::SessionError::Changed {
        log: "main".to_string(),
    };
    let expected = session_error.to_string();

    // -- Exec
    let error = MemoryError::backend(session_error);

    // -- Check
    assert_eq!(
        error.to_string(),
        format!("Memory backend error: {expected}")
    );
    let MemoryError::Backend(MemoryBackendError::Rig(inner)) = &error else {
        return Err(format!("a session error must wrap as Backend(Rig), got {error:?}").into());
    };
    assert_eq!(inner.to_string(), expected);
    assert_eq!(
        inner.to_string(),
        "the log `main` changed since it was read"
    );
    Ok(())
}

/// A `String` message wraps as `Backend(Message)`, and renders as the string.
#[test]
fn memory_error_backend_message_transparent() -> Result<()> {
    // -- Setup & Fixtures
    let message = format!("Compaction load error: {}", "the store refused");

    // -- Exec
    let error = MemoryError::backend(message.clone());

    // -- Check
    let MemoryError::Backend(MemoryBackendError::Message(inner)) = &error else {
        return Err(format!("a message must wrap as Backend(Message), got {error:?}").into());
    };
    assert_eq!(inner, &message);
    assert_eq!(
        error.to_string(),
        format!("Memory backend error: {message}")
    );
    Ok(())
}

/// The `Memory` arm carries the `MemoryError` through as it is: no nesting.
#[test]
fn memory_error_from_rig_store_memory_unwraps() -> Result<()> {
    // -- Setup & Fixtures
    let original = MemoryError::backend(String::from("boom"));
    let expected = original.to_string();

    // -- Exec
    let converted = MemoryError::from(RigStoreError::Memory(original));

    // -- Check
    assert_eq!(converted.to_string(), expected);
    let MemoryError::Backend(MemoryBackendError::Message(message)) = &converted else {
        return Err(
            format!("the memory arm must keep the Backend payload, got {converted:?}").into(),
        );
    };
    assert_eq!(message, "boom");
    Ok(())
}

/// The `Io` arm carries the `io::Error` into `Backend(Io)`.
#[test]
fn memory_error_from_rig_store_io() -> Result<()> {
    // -- Setup & Fixtures
    let io_error = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "no such file");
    let expected = io_error.to_string();

    // -- Exec
    let error = MemoryError::from(RigStoreError::Io(io_error));

    // -- Check
    let MemoryError::Backend(MemoryBackendError::Io(inner)) = &error else {
        return Err(format!("an io store error must wrap as Backend(Io), got {error:?}").into());
    };
    assert_eq!(inner.to_string(), expected);
    Ok(())
}

/// The `Sqlite` arm carries the `sqlx::Error` into `Backend(Sqlx)`.
#[test]
fn memory_error_from_rig_store_sqlite() -> Result<()> {
    // -- Setup & Fixtures
    let sqlx_error = sqlx::Error::RowNotFound;
    let expected = sqlx_error.to_string();

    // -- Exec
    let error = MemoryError::from(RigStoreError::Sqlite(sqlx_error));

    // -- Check
    let MemoryError::Backend(MemoryBackendError::Sqlx(inner)) = &error else {
        return Err(
            format!("a sqlite store error must wrap as Backend(Sqlx), got {error:?}").into(),
        );
    };
    assert_eq!(inner.to_string(), expected);
    Ok(())
}

/// The `Session` arm carries the rig `SessionError` into `Backend(Rig)`.
#[test]
fn memory_error_from_rig_store_session() -> Result<()> {
    // -- Setup & Fixtures
    let session_error = rig_memory::SessionError::OldLayout;
    let expected = session_error.to_string();

    // -- Exec
    let error = MemoryError::from(RigStoreError::Session(session_error));

    // -- Check
    let MemoryError::Backend(MemoryBackendError::Rig(inner)) = &error else {
        return Err(
            format!("a session store error must wrap as Backend(Rig), got {error:?}").into(),
        );
    };
    assert_eq!(inner.to_string(), expected);
    Ok(())
}
