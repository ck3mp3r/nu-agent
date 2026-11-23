//! Image blobs kept as files in the session's store, and the factory seam.

// region:    --- Modules

use crate::session::{RigStoreConfig, StoreType, create_rig_store_in};
use crate::types::Message;
use rig::completion::message::{DocumentSourceKind, ImageMediaType, UserContent};
use rig_memory::Files;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

// endregion: --- Modules

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Tests

/// An image is saved as a blob named by its content and read back.
#[tokio::test]
async fn test_rig_save_round_trips_an_image_as_a_blob() -> Result<()> {
    // -- Setup & Fixtures
    let store = RigStoreConfig::in_memory().open("session-blobs").await?;
    let bytes = b"hi";
    let name = format!("{}.png", hex(Sha256::digest(bytes).as_slice()));
    let mut conversation = rig_memory::load(&*store, "session-blobs")?;
    conversation.push(image_message(bytes));

    // -- Exec: the save writes the image's data as a blob
    rig_memory::save(&*store, "session-blobs", &mut conversation)?;

    // -- Check: the saved entry names the blob instead of holding the data
    let log = String::from_utf8(store.read_log("session-blobs")?)?;
    assert!(
        log.contains(&format!("blob:{name}")),
        "the saved entry must name the blob, got: {log}"
    );

    // -- Check: the blob file is there, named by the content
    assert_eq!(store.read_file(&format!("blobs/{name}"))?, bytes);
    assert!(
        store.files()?.contains(&format!("blobs/{name}")),
        "the blob is one of the session's files"
    );

    // -- Exec: reading the log puts the data back
    let restored = rig_memory::load(&*store, "session-blobs")?;

    // -- Check: the image holds the data again
    let entry = restored
        .entries()
        .next()
        .ok_or("the image message is in the conversation")?;
    let restored_image = entry.message.images().next().ok_or("the image is there")?;
    match &restored_image.data {
        DocumentSourceKind::Base64(data) => assert_eq!(data, "aGk="),
        other => return Err(format!("expected base64 data, got {other:?}").into()),
    }
    Ok(())
}

/// A file-backed store keeps the blob across engines.
#[tokio::test]
async fn test_rig_store_blobs_survive_a_reopened_store() -> Result<()> {
    // -- Setup & Fixtures
    let temp = TempDir::new()?;
    let bytes = b"hi";
    let name = format!("{}.png", hex(Sha256::digest(bytes).as_slice()));
    let rig = RigStoreConfig::new(StoreType::Sqlite).with_cache_dir(temp.path());
    let written = {
        let store = rig.open("session-blobs-reopened").await?;
        let mut conversation = rig_memory::load(&*store, "session-blobs-reopened")?;
        conversation.push(image_message(bytes));
        rig_memory::save(&*store, "session-blobs-reopened", &mut conversation)?;
        store.files()?
    };

    // -- Exec
    let reopened = rig.open("session-blobs-reopened").await?;

    // -- Check
    assert!(written.contains(&format!("blobs/{name}")));
    assert_eq!(reopened.read_file(&format!("blobs/{name}"))?, bytes);
    Ok(())
}

/// The factory seam opens its database at `{cache}/{APP_DIR_NAME}/sessions.db`.
#[test]
fn test_seam_sqlite_uses_the_session_database() -> Result<()> {
    // -- Setup & Fixtures
    let temp = TempDir::new()?;

    // -- Exec
    let store = create_rig_store_in(StoreType::Sqlite, temp.path(), "session-seam")?;
    store.append_log("agent", 0, b"one")?;

    // -- Check
    let path = crate::utils::paths::app_dir(temp.path()).join("sessions.db");
    assert!(
        path.is_file(),
        "the database is in the app directory: {path:?}"
    );
    assert_eq!(store.read_log("agent")?, b"one");
    Ok(())
}

/// The seam opens an in-memory store that writes nothing to disk.
#[test]
fn test_seam_memory_writes_nothing() -> Result<()> {
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

// endregion: --- Tests

// region:    --- Support

/// A user message that carries one PNG image of `bytes`.
fn image_message(bytes: &[u8]) -> Message {
    Message::User {
        content: vec![
            UserContent::text("look"),
            UserContent::image_raw(bytes.to_vec(), Some(ImageMediaType::PNG), None),
        ],
    }
}

/// `bytes` as lowercase hexadecimal.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

// endregion: --- Support
