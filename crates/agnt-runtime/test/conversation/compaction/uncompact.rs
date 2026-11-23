//! `/uncompact`: the undo of the newest compaction, its records, and its
//! refusal.
//!
//! rig-memory 0.47 has no undo of its own, so the undo is built from what the
//! store does keep: the entries each summary replaced, read from the log by the
//! summary's id. These tests pin the folded store state after it — the region the
//! newest summary replaced, back in that summary's place — and the two rows every
//! outcome publishes.

use agnt_kernel::bus::LifecycleEvent;
use agnt_kernel::{CompactionRecord, CompactionSource, NoticeKind, SessionRecord};
use rig_memory::SummaryState;

use crate::bus::Bus;
use crate::session::{CachedMemory, RigStoreConfig};
use crate::types::Message;

use super::uncompact;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

const CONV: &str = "conv-uncompact";

// region:    --- Test Support

/// A `CachedMemory` over an in-memory rig store: one store per session id, so
/// the test seeds through the memory's own handle and reads back what the undo
/// saved.
fn fixture() -> CachedMemory {
    CachedMemory::new(RigStoreConfig::in_memory())
}

/// Seed a short conversation: two exchanges.
async fn seed(memory: &CachedMemory) -> Result<()> {
    let store = memory.rig_store(CONV).await?;
    let mut conversation = rig_memory::load(&*store, CONV)?;
    conversation.extend([
        Message::user("first question"),
        Message::assistant("first answer"),
        Message::user("second question"),
        Message::assistant("second answer"),
    ]);
    rig_memory::save(&*store, CONV, &mut conversation)?;
    Ok(())
}

/// Write what a compaction that summarized the older half leaves: one summary in
/// place of the two older messages, which is the splice's whole record.
async fn land_summary(memory: &CachedMemory) -> Result<()> {
    let store = memory.rig_store(CONV).await?;
    let mut conversation = rig_memory::load(&*store, CONV)?;
    let replaces = conversation
        .messages()
        .get(..2)
        .ok_or("the seeded conversation holds the two older messages")?
        .to_vec();
    let state = SummaryState {
        summary: "summarized the older half".to_string(),
        ..SummaryState::default()
    };
    rig_memory::apply_summary(&mut conversation, &replaces, &state).ok_or("the summary lands")?;
    rig_memory::save(&*store, CONV, &mut conversation)?;
    Ok(())
}

/// Every text block an assistant message carries, joined.
fn assistant_text(message: &Message) -> String {
    let Message::Assistant(turn) = message else {
        return String::new();
    };
    turn.content
        .iter()
        .filter_map(|content| match content {
            crate::types::AssistantContent::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect()
}

/// The loaded conversation's messages, cloned out of the store.
async fn messages(memory: &CachedMemory) -> Result<Vec<Message>> {
    let store = memory.rig_store(CONV).await?;
    let conversation = rig_memory::load(&*store, CONV)?;
    Ok(conversation
        .messages()
        .iter()
        .map(|message| (**message).clone())
        .collect())
}

// endregion: --- Test Support

/// WHEN `/uncompact` runs on a compacted session, THE undo SHALL put the entries
/// the newest summary replaced back in its place and publish both rows.
#[tokio::test]
async fn uncompact_restores_the_newest_compaction_and_publishes_the_rows() -> Result<()> {
    // -- Setup & Fixtures
    let memory = fixture();
    seed(&memory).await?;
    land_summary(&memory).await?;
    let bus = Bus::default();
    let mut records = bus.stream().subscribe();
    let mut spine = bus.lifecycle().subscribe();

    // -- Exec
    uncompact(
        CONV,
        &memory,
        CompactionSource::Manual,
        bus.stream(),
        bus.lifecycle(),
    )
    .await
    .map_err(|e| format!("the undo should run: {e}"))?;

    // -- Check
    let restored = messages(&memory).await?;
    assert_eq!(
        restored.len(),
        4,
        "the two replaced messages are back and the summary is gone, got: {restored:?}"
    );
    assert_eq!(
        assistant_text(&restored[1]),
        "first answer",
        "the restored order is the order the compaction replaced"
    );
    let record = records.recv().await?;
    assert!(
        matches!(
            record,
            SessionRecord::Compaction(CompactionRecord::Uncompacted {
                source: CompactionSource::Manual,
            })
        ),
        "the undo publishes Uncompacted with the command's source, got: {record:?}"
    );
    let row = spine.recv().await?;
    assert!(
        matches!(
            row,
            LifecycleEvent::Uncompacted {
                source: CompactionSource::Manual
            }
        ),
        "the undo publishes its terminal spine row, got: {row:?}"
    );
    Ok(())
}

/// WHEN `/uncompact` runs on a session with no compaction, THE undo SHALL refuse
/// with the reason, publish the notice and the spine's failure row, and write
/// nothing.
#[tokio::test]
async fn uncompact_without_a_compaction_changes_nothing_and_reports_a_notice() -> Result<()> {
    // -- Setup & Fixtures
    let memory = fixture();
    seed(&memory).await?;
    let bus = Bus::default();
    let mut records = bus.stream().subscribe();
    let mut spine = bus.lifecycle().subscribe();

    // -- Exec
    let result = uncompact(
        CONV,
        &memory,
        CompactionSource::Manual,
        bus.stream(),
        bus.lifecycle(),
    )
    .await;

    // -- Check
    let Err(message) = result else {
        return Err("an undo with nothing to restore must refuse".into());
    };
    assert!(
        message.contains("no compaction to undo"),
        "the refusal names the cause, got: {message}"
    );
    let unchanged = messages(&memory).await?;
    assert_eq!(
        unchanged.len(),
        4,
        "a refused undo writes nothing, got: {unchanged:?}"
    );
    let session_record = records.recv().await?;
    let SessionRecord::Notice(notice) = session_record else {
        return Err("the refusal must publish a notice record".into());
    };
    assert_eq!(notice.kind, NoticeKind::Warning);
    assert!(
        notice.text.contains("no compaction to undo"),
        "the notice carries the reason, got: {}",
        notice.text
    );
    let row = spine.recv().await?;
    assert!(
        matches!(row, LifecycleEvent::UncompactFailed { .. }),
        "the refusal publishes the terminal spine row, got: {row:?}"
    );
    Ok(())
}
