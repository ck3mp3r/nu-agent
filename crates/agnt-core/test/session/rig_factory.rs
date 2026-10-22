//! Unit tests for the rig-memory store seam.

use super::{RigStoreError, StoreType, create_rig_store_in, session_dir};
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
