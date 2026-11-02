//!
//! The settle's key computation: a turn whose pre-turn log could not be read
//! records nothing, and never repoints the session's history.

use crate::session::settle::{FactStore, SettleInput, new_keys};
use crate::session::{RigStoreConfig, StoreType};
use agnt_storage::{Entries, MessageKey, ToolCalls, Turns};

pub(crate) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

const SESSION: &str = "test-settle-keys";

/// The fact tables of a fresh file store in `dir`.
fn facts_config(dir: &std::path::Path) -> RigStoreConfig {
    RigStoreConfig::new(StoreType::Sqlite).with_cache_dir(dir)
}

/// The keys a settled turn records: `k0` and `k1` of turn 1.
fn two_keys() -> [MessageKey; 2] {
    [
        MessageKey {
            made: 1_790_974_800_213,
            id: 0,
        },
        MessageKey {
            made: 1_790_974_800_214,
            id: 1,
        },
    ]
}

/// A turn whose pre-turn log could not be read records nothing, and the first
/// turn's own keys stay on it: history is never repointed.
#[tokio::test]
async fn settle_skips_the_unit_when_the_pre_turn_keys_are_unknown() -> Result<()> {
    // -- Setup & Fixtures: turn 1 settled with the first two keys
    let temp_dir = tempfile::tempdir()?;
    let rig = facts_config(temp_dir.path());
    let facts = FactStore::open(&rig).await?;
    let keys = two_keys();
    facts
        .settle(SettleInput {
            session_id: SESSION,
            model: "anthropic/claude-sonnet-4-5",
            agent: "tester",
            submitted_millis: 1_790_974_800_000,
            keys: Some(keys.to_vec()),
            records: &[],
        })
        .await?;

    // -- Exec: turn 2 with the pre-turn keys unknown. Its saved log is the whole
    // session, so passing them as a delta would claim turn 1's entries.
    facts
        .settle(SettleInput {
            session_id: SESSION,
            model: "anthropic/claude-sonnet-4-5",
            agent: "tester",
            submitted_millis: 1_790_974_900_000,
            keys: None,
            records: &[],
        })
        .await?;

    // -- Check: no second turn, and every key still points at turn 1
    let turns = Turns::new(rig.facts_blocking()?).read(SESSION).await?;
    assert_eq!(turns.len(), 1, "the unknown turn wrote no row: {turns:?}");
    assert_eq!(turns[0].turn_seq, 1);
    let entries = Entries::new(rig.facts_blocking()?).read(SESSION).await?;
    assert_eq!(entries.len(), keys.len(), "no key of history was repointed");
    assert!(
        entries.iter().all(|entry| entry.turn_seq == 1),
        "every key keeps its own turn: {entries:?}"
    );
    let calls = ToolCalls::new(rig.facts_blocking()?).read(SESSION).await?;
    assert!(calls.is_empty(), "no call row of a skipped turn: {calls:?}");
    Ok(())
}

/// The delta is named only when the "before" is known; an empty known log is a
/// real first turn and settles every saved key.
#[test]
fn new_keys_names_the_delta_only_when_the_before_is_known() {
    // -- Setup & Fixtures
    let keys = two_keys();
    let saved = keys.to_vec();

    // -- Exec & Check
    assert_eq!(
        new_keys(&saved, None),
        None,
        "an unread before yields no delta, not the whole log"
    );
    assert_eq!(
        new_keys(&saved, Some(&[])),
        Some(keys.to_vec()),
        "the first turn's empty log is a known before"
    );
    assert_eq!(
        new_keys(&saved, Some(&keys[..1])),
        Some(vec![keys[1]]),
        "only the keys beyond the known before"
    );
}
