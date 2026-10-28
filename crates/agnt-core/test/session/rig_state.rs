//! Tests for the host-written session state records and the listing cache.

use crate::session::rig_state::{title_of_messages, write_state_records};
use crate::session::{RigStoreConfig, StoreType};
use crate::types::Message;

use rig_memory::{Session, SessionMeta, Store};

use std::sync::Arc;
use tempfile::TempDir;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

const CONV: &str = "conv-1";

// region:    --- Test Support

/// An in-memory rig store holding a two-message conversation.
fn seeded_store() -> Result<Arc<dyn Store>> {
    let store = crate::session::RigStoreConfig::in_memory().open(CONV)?;
    let mut conversation = rig_memory::load(&*store, CONV)?;
    conversation.extend([
        Message::user("how do I read a file?"),
        Message::assistant("use the read tool"),
    ]);
    rig_memory::save(&*store, CONV, &mut conversation)?;
    Ok(store)
}

/// The main agent's log name.
fn main_log(session: &Session) -> Result<String> {
    session
        .main_agent()
        .map(|agent| agent.id.clone())
        .ok_or_else(|| "the session has a main agent".into())
}

// endregion: --- Test Support

#[test]
fn write_state_records_persists_model_and_title() -> Result<()> {
    // -- Setup & Fixtures
    let store = seeded_store()?;

    // -- Exec
    write_state_records(&*store, "openai/gpt-4o", Some("how do I read a file?"))?;

    // -- Check: the state records are readable through the typed accessors.
    let session = Session::read(&*store)?;
    let main = session.main_agent().ok_or("the session has a main agent")?;
    assert_eq!(main.model().as_deref(), Some("openai/gpt-4o"));
    assert_eq!(main.title().as_deref(), Some("how do I read a file?"));

    // -- Check: the listing carries every field it owns.
    let meta = SessionMeta::read(&*store).ok_or("the listing was written")?;
    assert_eq!(meta.title, "how do I read a file?");
    assert_eq!(meta.model, "openai/gpt-4o");
    assert_eq!(
        meta.messages, 2,
        "the main agent's active line holds 2 messages"
    );
    assert!(meta.updated > 0, "the listing records when it last changed");
    assert!(meta.labels.is_empty(), "labels are plugin-owned");
    assert!(meta.listed.is_empty(), "listed values are plugin-owned");
    Ok(())
}

#[test]
fn write_state_records_adds_no_record_for_unchanged_values() -> Result<()> {
    // -- Setup & Fixtures
    let store = seeded_store()?;
    write_state_records(&*store, "openai/gpt-4o", Some("a title"))?;
    let session = Session::read(&*store)?;
    let log = main_log(&session)?;
    let before = store.read_log(&log)?.len();

    // -- Exec: the same values again.
    write_state_records(&*store, "openai/gpt-4o", Some("a title"))?;

    // -- Check
    let after = store.read_log(&log)?.len();
    assert_eq!(
        after, before,
        "a repeated write with unchanged values must add no record"
    );
    Ok(())
}

#[test]
fn write_state_records_updates_the_model_on_switch() -> Result<()> {
    // -- Setup & Fixtures
    let store = seeded_store()?;
    write_state_records(&*store, "openai/gpt-4o", Some("a title"))?;

    // -- Exec
    write_state_records(&*store, "anthropic/claude-sonnet-4", Some("a title"))?;

    // -- Check
    let session = Session::read(&*store)?;
    let main = session.main_agent().ok_or("the session has a main agent")?;
    assert_eq!(main.model().as_deref(), Some("anthropic/claude-sonnet-4"));
    let meta = SessionMeta::read(&*store).ok_or("the listing was written")?;
    assert_eq!(
        meta.model, "anthropic/claude-sonnet-4",
        "the listing must follow the model switch"
    );
    Ok(())
}

#[test]
fn write_state_records_keeps_the_title_when_none_is_given() -> Result<()> {
    // -- Setup & Fixtures: a session that already carries a title.
    let store = seeded_store()?;
    write_state_records(&*store, "openai/gpt-4o", Some("the original title"))?;

    // -- Exec: a model switch passes no title.
    write_state_records(&*store, "anthropic/claude-sonnet-4", None)?;

    // -- Check: the model follows the switch, the title is untouched.
    let session = Session::read(&*store)?;
    let main = session.main_agent().ok_or("the session has a main agent")?;
    assert_eq!(main.model().as_deref(), Some("anthropic/claude-sonnet-4"));
    assert_eq!(
        main.title().as_deref(),
        Some("the original title"),
        "a write with no title must leave the saved title alone"
    );
    let meta = SessionMeta::read(&*store).ok_or("the listing was written")?;
    assert_eq!(
        meta.title, "the original title",
        "relist must keep the listing's title when no title is given"
    );
    Ok(())
}

#[test]
fn write_state_records_works_on_the_dir_backend() -> Result<()> {
    // -- Setup & Fixtures
    let temp_dir = TempDir::new().map_err(|_| "should create temp dir")?;
    let store = RigStoreConfig::new(StoreType::Jsonl)
        .with_cache_dir(temp_dir.path())
        .open(CONV)?;
    let mut conversation = rig_memory::load(&*store, CONV)?;
    conversation.extend([Message::user("hello"), Message::assistant("hi")]);
    rig_memory::save(&*store, CONV, &mut conversation)?;

    // -- Exec
    write_state_records(&*store, "openai/gpt-4o", Some("hello"))?;

    // -- Check
    let session = Session::read(&*store)?;
    let main = session.main_agent().ok_or("the session has a main agent")?;
    assert_eq!(main.model().as_deref(), Some("openai/gpt-4o"));
    let meta = SessionMeta::read(&*store).ok_or("the listing was written")?;
    assert_eq!(meta.title, "hello");
    assert_eq!(meta.messages, 2);
    Ok(())
}

#[test]
fn title_of_messages_takes_the_first_user_text() {
    // -- Setup & Fixtures
    let messages = vec![
        Message::assistant("a preamble reply"),
        Message::user("the first thing the user typed"),
        Message::user("a later turn"),
    ];

    // -- Exec
    let title = title_of_messages(&messages);

    // -- Check
    assert_eq!(title.as_deref(), Some("the first thing the user typed"));
    assert!(
        title_of_messages(&[Message::assistant("no user turn")]).is_none(),
        "a conversation with no user text has no title"
    );
}
