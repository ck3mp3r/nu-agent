//!
//! The session listing's facts: what the picker's When and Agent cells read.
//! Every row shows only what the store recorded — an unknown instant stays
//! blank, and the agent is the last recorded turn's.

use std::sync::Arc;

use super::harness::*;
use super::*;
use agnt_runtime::session::management::SessionPersistence;
use agnt_runtime::session::settle::SettleInput;

/// A runtime on a store that lives on disk: the facts one open writes are the
/// facts the next open reads. The harness's own config is in-memory, whose
/// database is minted per open.
fn sqlite_runtime() -> Result<RuntimeFixture> {
    let mut fixture = make_runtime(text_model("unused"))?;
    let cache = fixture.root().join("listing-facts");
    fixture.runtime.session = agnt_runtime::conversation::state::memory::MemoryState::new(
        agnt_runtime::session::RigStoreConfig::new(agnt_runtime::session::StoreType::Sqlite)
            .with_cache_dir(cache),
    );
    Ok(fixture)
}

/// The session id a fixture's cwd owns.
fn session_id(fixture: &RuntimeFixture, suffix: &str) -> String {
    format!(
        "{}-{suffix}",
        agnt_runtime::session::prefix::dir_prefix(fixture.root())
    )
}

/// Seed `id`'s log with `messages` and write its state records, so the listing
/// counts it.
async fn seed(
    fixture: &RuntimeFixture,
    id: &str,
    messages: &[crate::types::Message],
) -> Result<()> {
    let store = fixture
        .runtime
        .session
        .inner_memory()
        .rig_store(id)
        .await
        .map_err(|e| format!("rig store should open: {e:?}"))?;
    let mut conversation = rig_memory::load(&*store, id)?;
    conversation.extend(messages.iter().cloned());
    rig_memory::save(&*store, id, &mut conversation)?;
    agnt_runtime::session::rig_state::write_state_records(
        Arc::clone(&store),
        "test/model".to_string(),
        None,
    )
    .await?;
    Ok(())
}

/// Record one settled turn of `id` under `agent`.
///
/// The turn row is what the listing reads; it carries no entry keys here, so
/// the fixture never repoints the session's history.
async fn settle(
    fixture: &RuntimeFixture,
    id: &str,
    agent: &str,
    submitted_millis: i64,
) -> Result<()> {
    let facts = fixture
        .runtime
        .session
        .inner_memory()
        .facts()
        .await
        .map_err(|e| format!("facts should open: {e:?}"))?;
    facts
        .settle(SettleInput {
            session_id: id,
            model: "openai/gpt-5",
            agent,
            submitted_millis,
            keys: Some(Vec::new()),
            records: &[],
        })
        .await?;
    Ok(())
}

/// WHEN the session's last turn recorded an agent, THE listing SHALL carry it.
#[tokio::test]
async fn session_list_reads_the_last_turns_agent() -> Result<()> {
    // -- Setup & Fixtures
    let fixture = sqlite_runtime()?;
    let id = session_id(&fixture, "mixed");
    seed(&fixture, &id, &[crate::types::Message::user("one")]).await?;
    settle(&fixture, &id, "coder", 1_790_974_800_000).await?;
    settle(&fixture, &id, "reviewer", 1_790_974_900_000).await?;

    // -- Exec
    let sessions = fixture.runtime.list_sessions(fixture.root()).await?;

    // -- Check: the latest turn's agent wins; no summary of the two.
    let session = sessions
        .iter()
        .find(|session| session.id == id)
        .ok_or("the session must be listed")?;
    assert_eq!(session.agent.as_deref(), Some("reviewer"));
    Ok(())
}

/// WHEN the session holds no recorded turn, THE agent cell SHALL be blank.
#[tokio::test]
async fn session_list_blanks_the_agent_without_turns() -> Result<()> {
    // -- Setup & Fixtures
    let fixture = sqlite_runtime()?;
    let id = session_id(&fixture, "no-turns");
    seed(&fixture, &id, &[crate::types::Message::user("one")]).await?;

    // -- Exec
    let sessions = fixture.runtime.list_sessions(fixture.root()).await?;

    // -- Check
    let session = sessions
        .iter()
        .find(|session| session.id == id)
        .ok_or("the session must be listed")?;
    assert_eq!(session.agent, None, "no recorded turn, no agent");
    Ok(())
}

/// WHEN the last turn ran persona-less, THE agent cell SHALL be blank: the
/// recorded empty string is no identity.
#[tokio::test]
async fn session_list_blanks_a_persona_less_last_turn() -> Result<()> {
    // -- Setup & Fixtures
    let fixture = sqlite_runtime()?;
    let id = session_id(&fixture, "persona-less");
    seed(&fixture, &id, &[crate::types::Message::user("one")]).await?;
    settle(&fixture, &id, "coder", 1_790_974_800_000).await?;
    settle(&fixture, &id, "", 1_790_974_900_000).await?;

    // -- Exec
    let sessions = fixture.runtime.list_sessions(fixture.root()).await?;

    // -- Check
    let session = sessions
        .iter()
        .find(|session| session.id == id)
        .ok_or("the session must be listed")?;
    assert_eq!(session.agent, None, "the empty record is not an agent");
    Ok(())
}

/// WHEN the store recorded no change instant, THE row SHALL carry none — never
/// the wall clock; a recorded instant SHALL come through as itself.
#[tokio::test]
async fn session_list_leaves_an_unknown_updated_blank() -> Result<()> {
    // -- Setup & Fixtures: one session with no recorded instant, one with a real
    // one.
    let fixture = sqlite_runtime()?;
    let unknown = session_id(&fixture, "unknown-when");
    let known = session_id(&fixture, "known-when");
    seed(&fixture, &unknown, &[crate::types::Message::user("one")]).await?;
    seed(&fixture, &known, &[crate::types::Message::user("two")]).await?;
    let store = fixture
        .runtime
        .session
        .inner_memory()
        .rig_store(&known)
        .await
        .map_err(|e| format!("rig store should open: {e:?}"))?;
    let mut meta = rig_memory::SessionMeta::read(&*store).unwrap_or_default();
    meta.updated = 1_765_000_000_000;
    meta.messages = meta.messages.max(1);
    meta.write(&*store)?;
    let store = fixture
        .runtime
        .session
        .inner_memory()
        .rig_store(&unknown)
        .await
        .map_err(|e| format!("rig store should open: {e:?}"))?;
    let mut meta = rig_memory::SessionMeta::read(&*store).unwrap_or_default();
    meta.updated = 0;
    meta.messages = meta.messages.max(1);
    meta.write(&*store)?;

    // -- Exec
    let sessions = fixture.runtime.list_sessions(fixture.root()).await?;

    // -- Check
    let unknown_row = sessions
        .iter()
        .find(|session| session.id == unknown)
        .ok_or("the unknown-instant session must be listed")?;
    assert_eq!(
        unknown_row.last_active, None,
        "an unknown instant is blank, never the wall clock"
    );
    let known_row = sessions
        .iter()
        .find(|session| session.id == known)
        .ok_or("the known-instant session must be listed")?;
    assert_eq!(
        known_row.last_active,
        chrono::DateTime::from_timestamp_millis(1_765_000_000_000),
        "a recorded instant comes through as itself"
    );
    Ok(())
}
