//! `/uncompact`: rig-memory's undo, its records, and pass one's reply.

use std::sync::Arc;

use agnt_kernel::bus::LifecycleEvent;
use agnt_kernel::{CompactionRecord, CompactionSource, NoticeKind, SessionRecord};
use rig::message::Message;
use rig_memory::SummaryState;
use tempfile::TempDir;

use crate::bus::Bus;
use crate::conversation::compaction::supersede;
use crate::hook::chain::Supersede;
use crate::session::{CachedMemory, RigStoreConfig, StoreType};

use super::uncompact;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

const CONV: &str = "conv-uncompact";

/// The text pass one's summary call answered with: both the reply entry's own
/// text and the summary the splice lands.
const REPLY: &str = "rolled-up summary";

// region:    --- Test Support

/// A `CachedMemory` over a file-backed rig store rooted at `dir`.
///
/// Two opens over one directory see the same conversation, so the test can seed
/// through the memory's own handle and read back what the undo saved.
async fn fixture(dir: &TempDir) -> Result<CachedMemory> {
    Ok(CachedMemory::new(
        RigStoreConfig::new(StoreType::Sqlite).with_cache_dir(dir.path()),
    ))
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

/// Write what a compaction that summarized everything but the newest message
/// leaves: the summary in place of the older entries, with its aside.
async fn land_summary(memory: &CachedMemory) -> Result<()> {
    let store = memory.rig_store(CONV).await?;
    let mut conversation = rig_memory::load(&*store, CONV)?;
    let keep_from = conversation.id(2).ok_or("the third message has an id")?;
    let state = SummaryState {
        summary: "summarized the older half".to_string(),
        ..SummaryState::default()
    };
    rig_memory::apply_summary(&mut conversation, Some(keep_from), &state)
        .ok_or("the summary lands")?;
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
            rig::message::AssistantContent::Text(text) => Some(text.text.as_str()),
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

#[tokio::test]
async fn uncompact_restores_the_newest_compaction_and_publishes_the_rows() -> Result<()> {
    // -- Setup & Fixtures
    let dir = TempDir::new()?;
    let memory = fixture(&dir).await?;
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
        "the four summarized messages are back, got: {restored:?}"
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

#[tokio::test]
async fn uncompact_without_a_compaction_changes_nothing_and_reports_a_notice() -> Result<()> {
    // -- Setup & Fixtures
    let dir = TempDir::new()?;
    let memory = fixture(&dir).await?;
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
        panic!("an undo with nothing to restore must refuse, got: {result:?}");
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
    let record = records.recv().await?;
    let SessionRecord::Notice(notice) = record else {
        panic!("the refusal must publish a notice record, got: {record:?}");
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

#[tokio::test]
async fn uncompact_does_not_resurrect_pass_ones_internal_reply() -> Result<()> {
    // -- Setup & Fixtures
    // A compaction that keeps nothing sweeps pass one's own exchange into the
    // replaced set: the summary IS the reply, so the restore would put the
    // internal answer back. S3's clean-up never saw it — the sweep took it.
    let dir = TempDir::new()?;
    let memory = fixture(&dir).await?;
    let store = memory.rig_store(CONV).await?;
    let mut conversation = rig_memory::load(&*store, CONV)?;
    conversation.extend([
        Message::user("first question"),
        Message::assistant("first answer"),
        Message::user("second question"),
        Message::assistant(REPLY),
    ]);
    rig_memory::save(&*store, CONV, &mut conversation)?;
    let bus = Bus::default();
    let state = SummaryState {
        summary: REPLY.to_string(),
        ..SummaryState::default()
    };
    let landed = supersede::land(
        Supersede::planned(None, state, CompactionSource::Manual, REPLY.to_string()),
        Arc::clone(&store),
        CONV.to_string(),
        bus.stream(),
        bus.lifecycle(),
    )
    .await;
    assert!(
        matches!(landed, supersede::Landed::Summary(_)),
        "the in-place splice must land, got: {landed:?}"
    );

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
        3,
        "the real messages are back and pass one's reply is not, got: {restored:?}"
    );
    assert!(
        restored
            .iter()
            .all(|message| assistant_text(message) != REPLY),
        "no restored entry carries pass one's reply text, got: {restored:?}"
    );
    assert_eq!(
        assistant_text(&restored[2]),
        "",
        "the newest entry is the user's question, not an answer, got: {restored:?}"
    );
    Ok(())
}
