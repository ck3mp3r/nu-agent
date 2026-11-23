//! The between-runs splice: the side-call's summary becomes the stored
//! conversation's summary, between pass one's save and pass two's load.
//!
//! The tests pin the folded store state after the splice: the summary entry by
//! `summary_author`, the kept messages after it, and pass one's own exchange left
//! where the run saved it.

use std::sync::Arc;

use rig::conversation::Author;
use rig_memory::{Store, SummaryState, summarized_in_log, summary_author};

use crate::bus::Bus;
use crate::conversation::compaction::supersede::{Landed, land};
use crate::hook::chain::PendingSummary;
use crate::session::RigStoreConfig;
use crate::types::Message;
use agnt_kernel::bus::LifecycleEvent;

const CONV: &str = "conv-1";
const SUMMARY: &str = "the older turns, summarized";
const ANSWER: &str = "the turn's real answer";

// region:    --- Test Support

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// An in-memory rig store: a memory store resolves no path, so no ambient cache
/// directory is touched.
async fn memory_store() -> Result<Arc<dyn Store>> {
    Ok(RigStoreConfig::in_memory().open(CONV).await?)
}

/// Seed the store with a conversation as pass one leaves it: the older turns, the
/// turn's prompt, and the turn's own answer.
async fn seeded(store: &Arc<dyn Store>) -> Result<()> {
    let mut conversation = rig_memory::load(&**store, CONV)?;
    conversation.extend([
        Message::user("an older question"),
        Message::assistant("an older answer"),
        Message::user("the turn's prompt"),
        Message::assistant(ANSWER),
    ]);
    rig_memory::save(&**store, CONV, &mut conversation)?;
    Ok(())
}

/// What a side-call stashes as replaced: the prompt is the first message kept,
/// so the older turns are what the summary replaces.
async fn replaces(store: &Arc<dyn Store>) -> Result<Vec<Arc<Message>>> {
    let conversation = rig_memory::load(&**store, CONV)?;
    let at = conversation
        .messages()
        .iter()
        .position(|message| user_text(message) == "the turn's prompt")
        .ok_or("the seeded conversation holds the turn's prompt")?;
    Ok(conversation
        .messages()
        .get(..at)
        .unwrap_or_default()
        .to_vec())
}

/// The summary the side-call produced, taken from the slot it wrote it to.
fn settled(pending: &PendingSummary) -> Result<crate::hook::chain::Supersede> {
    pending
        .take_settled()
        .ok_or_else(|| "the side-call's summary must be stashed for the executor".into())
}

/// Cut the store's conversation down to its first message, as another component
/// saving over the log leaves it: fewer messages than the side-call read, so the
/// prefix it summarized is no longer there to splice.
fn shorten(store: &dyn Store) -> Result<()> {
    let mut conversation = rig_memory::load(store, CONV)?;
    conversation.splice(1.., std::iter::empty(), Author::plugin("test"));
    rig_memory::save(store, CONV, &mut conversation)?;
    Ok(())
}

/// The concatenated user text of `message`.
fn user_text(message: &Message) -> String {
    let Message::User { content } = message else {
        return String::new();
    };
    content
        .iter()
        .filter_map(|item| match item {
            crate::types::UserContent::Text(text) => Some(text.text.clone()),
            _ => None,
        })
        .collect()
}

/// The concatenated assistant text of `message`.
fn assistant_text(message: &Message) -> String {
    let Message::Assistant(turn) = message else {
        return String::new();
    };
    turn.content
        .iter()
        .filter_map(|item| match item {
            crate::types::AssistantContent::Text(text) => Some(text.text.clone()),
            _ => None,
        })
        .collect()
}

// endregion: --- Test Support

/// WHEN the side-call produced a summary, THE between-runs splice SHALL leave the
/// folded store conversation leading with that summary under `summary_author`,
/// keeping the turn's prompt and pass one's own answer.
#[tokio::test]
async fn side_call_summary_lands_as_the_stored_summary() -> Result<()> {
    // -- Setup & Fixtures: pass one's exchange, already saved.
    let store = memory_store().await?;
    seeded(&store).await?;
    let bus = Bus::default();
    let mut rx = bus.lifecycle().subscribe();
    let pending_summary = PendingSummary::default();

    // -- Exec: the side-call stashes, the executor splices.
    let replaces = replaces(&store).await?;
    pending_summary.stash_side_call(
        replaces,
        SummaryState::default(),
        agnt_kernel::CompactionSource::Auto,
        SUMMARY.to_string(),
    );
    let landed = land(
        settled(&pending_summary)?,
        Arc::clone(&store),
        CONV.to_string(),
        bus.stream(),
        bus.lifecycle(),
    )
    .await;

    // -- Check: the summary leads, the prompt and the turn's own answer are kept.
    assert!(
        matches!(&landed, Landed::Summary(text) if text == SUMMARY),
        "the side-call's summary must land as the summary, got {landed:?}"
    );
    let conversation = rig_memory::load(&*store, CONV)?;
    let first = conversation
        .entries()
        .next()
        .ok_or("the compacted conversation is non-empty")?;
    assert_eq!(
        first.author,
        Some(&summary_author()),
        "the folded conversation must lead with the rig.compaction summary"
    );
    assert!(
        user_text(first.message).contains(SUMMARY),
        "the leading entry must carry the side-call's summary, got {:?}",
        user_text(first.message)
    );
    assert!(
        conversation
            .messages()
            .iter()
            .any(|message| user_text(message) == "the turn's prompt"),
        "the summary must keep the turn's prompt"
    );
    assert!(
        conversation
            .messages()
            .iter()
            .any(|message| assistant_text(message) == ANSWER),
        "the summary must keep pass one's own answer"
    );
    let recorded = summarized_in_log(&*store, CONV)?;
    assert_eq!(
        recorded.len(),
        1,
        "the splice must be the log's one summary, got {recorded:?}"
    );
    let (summary_id, replaced) = recorded
        .iter()
        .next()
        .ok_or("the log holds the splice's summary")?;
    assert_eq!(
        first.id, *summary_id,
        "the log's summary must be the entry the conversation leads with"
    );
    assert_eq!(
        replaced.len(),
        2,
        "the summary must record the two older turns it replaced"
    );
    let completed = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .map_err(|_| "should receive CompactionCompleted within timeout")?
        .map_err(|_| "should receive CompactionCompleted")?;
    assert!(
        matches!(&completed, LifecycleEvent::CompactionCompleted { summary, .. } if summary == SUMMARY),
        "the splice must announce the summary it landed, got {completed:?}"
    );
    Ok(())
}

/// WHEN the stored conversation no longer keeps the prefix the side-call read,
/// THE splice SHALL be refused and the log SHALL gain no summary.
#[tokio::test]
async fn refused_splice_leaves_no_summary_in_the_log() -> Result<()> {
    // -- Setup & Fixtures: the side-call's stash, then a conversation shorter
    // than the prefix it read.
    let store = memory_store().await?;
    seeded(&store).await?;
    let bus = Bus::default();
    let pending_summary = PendingSummary::default();
    let replaces = replaces(&store).await?;
    pending_summary.stash_side_call(
        replaces,
        SummaryState::default(),
        agnt_kernel::CompactionSource::Auto,
        SUMMARY.to_string(),
    );
    let before = summarized_in_log(&*store, CONV)?;
    assert!(
        before.is_empty(),
        "nothing is in the log as a summary before the splice, got {before:?}"
    );
    shorten(&*store)?;

    // -- Exec: the executor splices over the shortened conversation.
    let landed = land(
        settled(&pending_summary)?,
        Arc::clone(&store),
        CONV.to_string(),
        bus.stream(),
        bus.lifecycle(),
    )
    .await;

    // -- Check: the splice is refused and the log keeps its summaries.
    assert!(
        matches!(
            &landed,
            Landed::Rejected(message) if message == "Compaction summary could not be spliced in"
        ),
        "a conversation that no longer keeps the prefix must refuse the splice, got {landed:?}"
    );
    assert_eq!(
        summarized_in_log(&*store, CONV)?,
        before,
        "a refused splice must leave the log without a new summary"
    );
    Ok(())
}

/// WHEN a side-call stashes a summary, THE stash SHALL spend the turn's one
/// attempt, and the turn-end sweep SHALL leave nothing for the next turn.
#[tokio::test]
async fn one_attempt_per_turn_side_call() -> Result<()> {
    // -- Setup & Fixtures: a slot that already stashed a summary.
    let store = memory_store().await?;
    seeded(&store).await?;
    let pending_summary = PendingSummary::default();
    let replaces = replaces(&store).await?;
    pending_summary.stash_side_call(
        replaces,
        SummaryState::default(),
        agnt_kernel::CompactionSource::Auto,
        SUMMARY.to_string(),
    );

    // -- Exec & Check: the attempt is spent until the turn-end sweep.
    assert!(
        pending_summary.attempted(),
        "a stashed summary spends the turn's one attempt"
    );
    pending_summary.clear();
    assert!(
        !pending_summary.attempted() && pending_summary.take_settled().is_none(),
        "the turn-end sweep must leave nothing for the next turn"
    );
    Ok(())
}
