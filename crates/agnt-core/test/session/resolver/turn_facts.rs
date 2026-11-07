//!
//! The reload fill: a user row carries the facts its turn recorded — the true
//! submit instant and the identities the turn ran under — and nothing when the
//! join misses or the record cannot stand behind a footer.

use super::*;
use agnt_runtime::session::settle::{FactStore, SettleInput};
use agnt_runtime::session::{RigStoreConfig, StoreType};

/// The submit instant the fixture recorded. Deliberately far from the settle
/// instant, so a reload that re-derived its stamp would show the wrong one.
const SUBMITTED: i64 = 1_790_974_800_000;

/// Seed `session_id`'s log with `messages` and answer the keys rig gave them.
async fn seed(
    rig: &RigStoreConfig,
    session_id: &str,
    messages: Vec<Message>,
) -> Result<Vec<agnt_storage::MessageKey>> {
    let store = rig.open(session_id).await?;
    let mut conversation = rig_memory::load(&*store, session_id)?;
    conversation.extend(messages);
    rig_memory::save(&*store, session_id, &mut conversation)?;
    let saved = rig_memory::load(&*store, session_id)?;
    Ok(agnt_runtime::session::settle::entry_keys(saved.entries()))
}

/// WHEN the reload join hits a settled turn, THE user row SHALL carry that
/// turn's recorded submit instant and identities verbatim.
#[tokio::test]
async fn reload_stamps_the_recorded_submit_instant_and_identities() -> Result<()> {
    // -- Setup & Fixtures: one settled turn, recorded under a real submit instant
    let temp_dir = tempfile::tempdir()?;
    let rig = RigStoreConfig::new(StoreType::Sqlite).with_cache_dir(temp_dir.path().join("cache"));
    let session_id = "test-turn-facts";
    let keys = seed(
        &rig,
        session_id,
        vec![Message::user("the prompt"), Message::assistant("the reply")],
    )
    .await?;
    let facts = FactStore::open(&rig).await?;
    facts
        .settle(SettleInput {
            session_id,
            model: "anthropic/claude-sonnet-4-5",
            agent: "tester",
            submitted_millis: SUBMITTED,
            keys: Some(keys.clone()),
            records: &[],
        })
        .await?;
    let settled_free_start = 1_791_000_000_000i64;

    // -- Exec
    let store = rig.open(session_id).await?;
    let (entries, statuses, turns) =
        crate::session::resolver::read_entries(&rig, &store, session_id).await?;
    let snapshots = crate::session::resolver::hydrate_transcript_messages(
        &entries,
        std::path::Path::new("/tmp"),
        &statuses,
        &turns,
    );

    // -- Check: the prompt carries the submit instant, never the settle instant
    let user = snapshots
        .iter()
        .find(|snapshot| snapshot.role() == "user")
        .ok_or("the prompt must hydrate")?;
    // The settle instant is the moment the fixture ran, far above SUBMITTED: a
    // reload that re-derived its stamp from the settle would miss this by days.
    assert_eq!(user.stamp_millis(), Some(SUBMITTED));
    assert_eq!(user.stamp_model(), Some("anthropic/claude-sonnet-4-5"));
    assert_eq!(user.stamp_agent(), Some("tester"));
    assert!(
        user.stamp_millis()
            .is_some_and(|millis| millis < settled_free_start),
        "the recorded submit instant, never the settle instant"
    );

    // -- Check: the assistant row carries nothing
    let assistant = snapshots
        .iter()
        .find(|snapshot| snapshot.role() == "assistant")
        .ok_or("the reply must hydrate")?;
    assert_eq!(assistant.stamp_millis(), None);
    assert_eq!(assistant.stamp_model(), None);
    assert_eq!(assistant.stamp_agent(), None);
    Ok(())
}

/// WHEN the store records no turn for the entry, THE row SHALL carry no facts.
#[tokio::test]
async fn reload_of_a_fact_less_store_carries_nothing() -> Result<()> {
    // -- Setup & Fixtures: a log with no settled turn behind it
    let temp_dir = tempfile::tempdir()?;
    let rig = RigStoreConfig::new(StoreType::Sqlite).with_cache_dir(temp_dir.path().join("cache"));
    let session_id = "test-turn-facts-none";
    let _ = seed(&rig, session_id, vec![Message::user("the prompt")]).await?;

    // -- Exec
    let store = rig.open(session_id).await?;
    let (entries, statuses, turns) =
        crate::session::resolver::read_entries(&rig, &store, session_id).await?;
    let snapshots = crate::session::resolver::hydrate_transcript_messages(
        &entries,
        std::path::Path::new("/tmp"),
        &statuses,
        &turns,
    );

    // -- Check
    let user = snapshots
        .iter()
        .find(|snapshot| snapshot.role() == "user")
        .ok_or("the prompt must hydrate")?;
    assert_eq!(user.stamp_millis(), None, "no turn, no instant");
    assert_eq!(user.stamp_model(), None);
    assert_eq!(user.stamp_agent(), None);
    Ok(())
}

/// WHEN the joined turn's recorded instant cannot stand behind a footer, THE
/// row SHALL carry no facts at all — never a partial row.
#[tokio::test]
async fn reload_of_a_zero_stamp_carries_nothing() -> Result<()> {
    // -- Setup & Fixtures: a turn recorded with the unknown instant
    let temp_dir = tempfile::tempdir()?;
    let rig = RigStoreConfig::new(StoreType::Sqlite).with_cache_dir(temp_dir.path().join("cache"));
    let session_id = "test-turn-facts-zero";
    let keys = seed(&rig, session_id, vec![Message::user("the prompt")]).await?;
    let facts = FactStore::open(&rig).await?;
    facts
        .settle(SettleInput {
            session_id,
            model: "anthropic/claude-sonnet-4-5",
            agent: "tester",
            submitted_millis: 0,
            keys: Some(keys),
            records: &[],
        })
        .await?;

    // -- Exec
    let store = rig.open(session_id).await?;
    let (entries, statuses, turns) =
        crate::session::resolver::read_entries(&rig, &store, session_id).await?;
    let snapshots = crate::session::resolver::hydrate_transcript_messages(
        &entries,
        std::path::Path::new("/tmp"),
        &statuses,
        &turns,
    );

    // -- Check
    let user = snapshots
        .iter()
        .find(|snapshot| snapshot.role() == "user")
        .ok_or("the prompt must hydrate")?;
    assert_eq!(
        user.stamp_millis(),
        None,
        "an unknown instant makes the whole footer absent"
    );
    assert_eq!(user.stamp_model(), None, "no partial row");
    assert_eq!(user.stamp_agent(), None, "no partial row");
    Ok(())
}

/// The user snapshot a reload produces for a turn recorded with `model` and
/// `agent`: the tempdir store stands in for data recorded before the gate.
async fn reloaded_user(
    model: &str,
    agent: &str,
) -> Result<agnt_runtime::protocol::contracts::UiMessageSnapshot> {
    let temp_dir = tempfile::tempdir()?;
    let rig = RigStoreConfig::new(StoreType::Sqlite).with_cache_dir(temp_dir.path().join("cache"));
    let session_id = "test-turn-facts-gate";
    let keys = seed(&rig, session_id, vec![Message::user("the prompt")]).await?;
    let facts = FactStore::open(&rig).await?;
    facts
        .settle(SettleInput {
            session_id,
            model,
            agent,
            submitted_millis: SUBMITTED,
            keys: Some(keys),
            records: &[],
        })
        .await?;
    let store = rig.open(session_id).await?;
    let (entries, statuses, turns) =
        crate::session::resolver::read_entries(&rig, &store, session_id).await?;
    let snapshots = crate::session::resolver::hydrate_transcript_messages(
        &entries,
        std::path::Path::new("/tmp"),
        &statuses,
        &turns,
    );
    snapshots
        .into_iter()
        .find(|snapshot| snapshot.role() == "user")
        .ok_or_else(|| "the prompt must hydrate".into())
}

/// WHEN the joined turn recorded an empty agent, THE row SHALL carry no agent
/// segment — never `Some("")`, never a dangling separator.
#[tokio::test]
async fn reload_gates_an_empty_agent_to_no_segment() -> Result<()> {
    // -- Exec
    let user = reloaded_user("openai/gpt-5", "").await?;

    // -- Check
    assert_eq!(user.stamp_millis(), Some(SUBMITTED));
    assert_eq!(user.stamp_model(), Some("openai/gpt-5"));
    assert_eq!(
        user.stamp_agent(),
        None,
        "an empty recorded identity is unknown, not a segment"
    );
    Ok(())
}

/// WHEN the joined turn recorded a whitespace-only agent, THE row SHALL carry
/// no agent segment either.
#[tokio::test]
async fn reload_gates_a_whitespace_agent_to_no_segment() -> Result<()> {
    // -- Exec
    let user = reloaded_user("openai/gpt-5", "   ").await?;

    // -- Check
    assert_eq!(user.stamp_millis(), Some(SUBMITTED));
    assert_eq!(user.stamp_model(), Some("openai/gpt-5"));
    assert_eq!(user.stamp_agent(), None, "whitespace is not an identity");
    Ok(())
}

/// WHEN the joined turn recorded an empty model, THE row SHALL carry no model
/// segment on the same gate.
#[tokio::test]
async fn reload_gates_an_empty_model_to_no_segment() -> Result<()> {
    // -- Exec
    let user = reloaded_user("", "coder").await?;

    // -- Check
    assert_eq!(user.stamp_millis(), Some(SUBMITTED));
    assert_eq!(
        user.stamp_model(),
        None,
        "an empty recorded model is unknown"
    );
    assert_eq!(user.stamp_agent(), Some("coder"));
    Ok(())
}

/// WHEN the recorded identities are real text, THE row SHALL carry them
/// verbatim: the gate is emptiness only, never a trim.
#[tokio::test]
async fn reload_keeps_real_identities_untrimmed() -> Result<()> {
    // -- Exec
    let user = reloaded_user(" openai/gpt-5", " coder ").await?;

    // -- Check
    assert_eq!(user.stamp_model(), Some(" openai/gpt-5"));
    assert_eq!(user.stamp_agent(), Some(" coder "));
    Ok(())
}
