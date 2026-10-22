//! Unit tests for the rig-memory `DirStore` wiring.

use super::rig_dir_store::open_session_store;
use super::{NAME_FILE, RigStoreError, read_session_name, session_dir, write_session_name};
use crate::types::Message;
use rig::completion::message::{DocumentSourceKind, ImageMediaType, UserContent};
use rig_memory::{Files, LogWriteError, Store, load_images, store_images};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// The session directory holds `logs/` and `files/`.
#[test]
fn session_directory_layout() -> Result<()> {
    // -- Setup & Fixtures
    let temp = TempDir::new()?;
    let store = open_session_store(temp.path(), "session-layout")?;

    // -- Exec
    store.append_log("agent", 0, b"one")?;
    store.write_file("notes.txt", b"note")?;

    // -- Check
    let dir = session_dir(temp.path(), "session-layout");
    assert!(dir.join("logs").join("agent.jsonl").is_file());
    assert!(dir.join("files").join("notes.txt").is_file());
    Ok(())
}

/// The `name` file holds the display name of the session.
#[test]
fn name_file_round_trips() -> Result<()> {
    // -- Setup & Fixtures
    let temp = TempDir::new()?;
    let store = open_session_store(temp.path(), "session-name")?;

    // -- Exec & Check: a session without a name has none
    assert_eq!(read_session_name(&store)?, None);

    // -- Exec & Check: the name is written into the session's files
    write_session_name(&store, "Fix the parser")?;
    assert_eq!(
        read_session_name(&store)?,
        Some("Fix the parser".to_owned())
    );
    let path = session_dir(temp.path(), "session-name")
        .join("files")
        .join(NAME_FILE);
    assert_eq!(std::fs::read_to_string(path)?, "Fix the parser");
    Ok(())
}

/// A log write names the length it read and is refused when the log moved on.
#[test]
fn append_log_refuses_stale_length() -> Result<()> {
    // -- Setup & Fixtures
    let temp = TempDir::new()?;
    let store = open_session_store(temp.path(), "session-stale")?;
    store.append_log("agent", 0, b"abc")?;

    // -- Exec
    let refused = store.append_log("agent", 2, b"x");

    // -- Check
    assert!(matches!(refused, Err(LogWriteError::Changed)));
    assert_eq!(store.read_log("agent")?, b"abc");
    Ok(())
}

/// A session the JSONL store nu-agent kept before rig-memory saved is refused.
#[test]
fn legacy_jsonl_session_is_refused() -> Result<()> {
    // -- Setup & Fixtures
    let temp = TempDir::new()?;
    let sessions = crate::utils::paths::app_dir(temp.path()).join("sessions");
    std::fs::create_dir_all(&sessions)?;
    std::fs::write(sessions.join("session-legacy.jsonl"), b"{}\n")?;

    // -- Exec
    let refused = open_session_store(temp.path(), "session-legacy");

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

/// A session directory that holds a top-level JSONL file is refused.
#[test]
fn legacy_jsonl_in_session_directory_is_refused() -> Result<()> {
    // -- Setup & Fixtures
    let temp = TempDir::new()?;
    let dir = session_dir(temp.path(), "session-old");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("agent.jsonl"), b"{}\n")?;

    // -- Exec
    let refused = open_session_store(temp.path(), "session-old");

    // -- Check
    let error = refused.err().ok_or("a legacy session is refused")?;
    assert!(matches!(&error, RigStoreError::Legacy));
    Ok(())
}

/// An image is saved as a blob file under `files/blobs/` and read back.
#[test]
fn images_round_trip_as_blobs() -> Result<()> {
    // -- Setup & Fixtures
    let temp = TempDir::new()?;
    let store = open_session_store(temp.path(), "session-blobs")?;
    let bytes = b"hi";
    let message = Message::User {
        content: vec![
            UserContent::text("look"),
            UserContent::image_raw(bytes.to_vec(), Some(ImageMediaType::PNG), None),
        ],
    };
    let digest = Sha256::digest(bytes);
    let name = format!(
        "{}.png",
        digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );

    // -- Exec: the saved message names the blob instead of holding the data
    let saved = store_images(&message, &store)?;

    // -- Check: the saved image names the blob
    let saved_image = saved.images().next().ok_or("the image is there")?;
    match &saved_image.data {
        DocumentSourceKind::Url(url) => assert_eq!(url, &format!("blob:{name}")),
        other => return Err(format!("expected a blob url, got {other:?}").into()),
    }

    // -- Check: the blob file is there, named by the content
    let blob = session_dir(temp.path(), "session-blobs")
        .join("files")
        .join("blobs")
        .join(&name);
    assert_eq!(std::fs::read(&blob)?, bytes, "the blob holds the image");

    // -- Exec: reading the message puts the data back
    let mut restored = saved.into_owned();
    load_images(&mut restored, &store)?;

    // -- Check: the image holds the data again
    let restored_image = restored.images().next().ok_or("the image is there")?;
    match &restored_image.data {
        DocumentSourceKind::Base64(data) => assert_eq!(data, "aGk="),
        other => return Err(format!("expected base64 data, got {other:?}").into()),
    }
    Ok(())
}

/// A session id that leaves the sessions directory is refused.
#[test]
fn unsafe_session_id_is_refused() -> Result<()> {
    // -- Setup & Fixtures
    let temp = TempDir::new()?;

    // -- Exec
    let refused = open_session_store(temp.path(), "../elsewhere");

    // -- Check
    let error = refused.err().ok_or("an unsafe session id is refused")?;
    assert!(matches!(&error, RigStoreError::Io(_)));
    Ok(())
}
