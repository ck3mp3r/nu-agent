//!
//! The reload join: every entry names the turn that persisted it by rig's key,
//! so a fold, a cancel or a re-save never shifts the attribution.

use super::*;
use agnt_runtime::session::settle::{FactStore, SettleInput};
use agnt_runtime::session::{RigStoreConfig, StoreType};

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

/// The entries load with the turn each key was recorded under, and an entry no
/// turn holds stays unattributed.
#[tokio::test]
async fn reload_joins_every_entry_to_its_turn_by_key() -> Result<()> {
    // -- Setup & Fixtures: four entries, the first two settled under turn 1
    let temp_dir = tempfile::tempdir()?;
    let rig = RigStoreConfig::new(StoreType::Sqlite).with_cache_dir(temp_dir.path().join("cache"));
    let session_id = "test-turn-linkage";
    let keys = seed(
        &rig,
        session_id,
        vec![
            Message::user("q1"),
            Message::assistant("a1"),
            Message::user("q2"),
            Message::assistant("a2"),
        ],
    )
    .await?;
    assert_eq!(keys.len(), 4, "four entries, four keys");
    let facts = FactStore::open(&rig).await?;
    facts
        .settle(SettleInput {
            session_id,
            model: "anthropic/claude-sonnet-4-5",
            agent: "tester",
            submitted_millis: 1_790_974_800_000,
            keys: Some(vec![keys[0], keys[1]]),
            records: &[],
        })
        .await?;

    // -- Exec
    let store = rig.open(session_id).await?;
    let (entries, _, _) = crate::session::resolver::read_entries(&rig, &store, session_id).await?;

    // -- Check
    assert_eq!(entries.len(), 4);
    assert_eq!(entries[0].turn_seq, Some(1), "the prompt belongs to turn 1");
    assert_eq!(entries[1].turn_seq, Some(1), "the reply belongs to turn 1");
    assert_eq!(
        entries[2].turn_seq, None,
        "an entry no turn recorded is never attributed"
    );
    assert_eq!(entries[3].turn_seq, None);
    Ok(())
}

/// A fold that drops the entries between keeps every remaining entry on its own
/// turn: the join reads keys, never offsets.
#[tokio::test]
async fn reload_keeps_the_join_after_the_log_folds() -> Result<()> {
    // -- Setup & Fixtures: two settled turns, then the log folds to the second
    let temp_dir = tempfile::tempdir()?;
    let rig = RigStoreConfig::new(StoreType::Sqlite).with_cache_dir(temp_dir.path().join("cache"));
    let session_id = "test-turn-linkage-fold";
    let keys = seed(
        &rig,
        session_id,
        vec![Message::user("q1"), Message::assistant("a1")],
    )
    .await?;
    let facts = FactStore::open(&rig).await?;
    facts
        .settle(SettleInput {
            session_id,
            model: "anthropic/claude-sonnet-4-5",
            agent: "tester",
            submitted_millis: 1_790_974_800_000,
            keys: Some(keys.clone()),
            records: &[],
        })
        .await?;
    let keys = seed(
        &rig,
        session_id,
        vec![Message::user("q2"), Message::assistant("a2")],
    )
    .await?;
    facts
        .settle(SettleInput {
            session_id,
            model: "anthropic/claude-sonnet-4-5",
            agent: "tester",
            submitted_millis: 1_790_974_900_000,
            keys: Some(keys[2..].to_vec()),
            records: &[],
        })
        .await?;

    // -- Exec: the log now holds only the later pair
    let store = rig.open(session_id).await?;
    let mut conversation = rig_memory::load(&*store, session_id)?;
    conversation.splice(0..2, Vec::new(), rig_memory::summary_author());
    rig_memory::save(&*store, session_id, &mut conversation)?;
    let (entries, _, _) = crate::session::resolver::read_entries(&rig, &store, session_id).await?;

    // -- Check: the two remaining entries still carry turn 2
    assert_eq!(entries.len(), 2);
    assert!(
        entries.iter().all(|entry| entry.turn_seq == Some(2)),
        "a fold keeps the keys, so the join still answers: {entries:?}"
    );
    Ok(())
}
