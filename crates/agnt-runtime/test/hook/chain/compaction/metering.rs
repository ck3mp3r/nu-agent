//! Anchor metering: the entry the decision records and the tokens the next
//! decision reads back from it.

use super::super::*;
use super::*;

use rig::conversation::Conversation;
use rig_memory::request_tokens;

/// WHEN the decision meters, THE decision SHALL record the entry the request
/// counts through, so the next decision can anchor on the usage a completion
/// reports.
#[tokio::test]
async fn decide_compaction_records_the_request_and_the_metered_entry() -> Result<()> {
    // -- Setup & Fixtures: two stored entries, so the prompt becomes the third.
    let fx = decide_fixture()?;
    seed_rig(
        &fx,
        &[
            Message::user("turn one"),
            Message::assistant("turn one reply"),
        ],
    )
    .await?;

    // -- Exec
    decide_compaction(
        &Message::user("hi"),
        "conv-1",
        Some(&fx.spec),
        &fx.pending,
        &CompactionPath::new(
            fx.memory.as_ref(),
            &fx.compaction,
            &fx.meter,
            &Default::default(),
            &fx.model,
        ),
        fx.bus.stream(),
        fx.bus.lifecycle(),
    )
    .await;

    // -- Check: no usage has landed yet, so nothing is anchored; once one does, it
    // anchors through the prompt's entry — the store's two entries plus the prompt.
    assert!(
        fx.meter.anchor().is_none(),
        "no usage has been reported, so nothing is anchored yet"
    );
    fx.meter.record_usage(usage(1_000, 10));
    let anchor = fx
        .meter
        .anchor()
        .ok_or("the recorded entry anchors the usage")?;
    let store = fx
        .memory
        .rig_store("conv-1")
        .await
        .map_err(|e| format!("rig store should open: {e:?}"))?;
    let conversation = rig_memory::load(&*store, "conv-1")?;
    assert_eq!(
        anchor.through.get(),
        conversation.messages().len() as u64,
        "the usage counts through the prompt's entry, which the request appended"
    );
    assert_eq!(
        anchor.usage.context_tokens(),
        Some(1_010),
        "the anchor carries the reported usage"
    );
    Ok(())
}

/// WHEN an anchor names an entry in the conversation, THE count SHALL be the
/// reported input tokens plus an estimate of the messages after that entry.
#[test]
fn metering_anchors_on_the_reported_tokens_plus_what_follows() -> Result<()> {
    // -- Setup & Fixtures: two entries, anchored through the first, so the second
    // is what the anchor's estimate adds.
    let mut conversation = Conversation::new();
    conversation.extend([Message::user("older"), Message::assistant("reply")]);
    let at = conversation
        .index_of(conversation.id(0).ok_or("the first entry has an id")?)
        .ok_or("the first entry is in the conversation")?;

    // -- Exec: anchored through the first entry, and through the last one.
    let anchored = request_tokens((None, None), conversation.messages(), Some((5_000, at)));
    let at_last = request_tokens(
        (None, None),
        conversation.messages(),
        Some((5_000, conversation.messages().len() - 1)),
    );

    // -- Check
    assert!(
        anchored > at_last,
        "the estimate of the messages after the anchor is added: {anchored} vs {at_last}"
    );
    Ok(())
}

/// WHEN the anchor names the last entry of the conversation, THE count SHALL be
/// exactly the reported input tokens: nothing follows what the provider counted.
#[test]
fn metering_anchored_at_the_last_entry_adds_nothing() -> Result<()> {
    // -- Setup & Fixtures: two entries, anchored through the second.
    let mut conversation = Conversation::new();
    conversation.extend([Message::user("older"), Message::assistant("reply")]);
    let last = conversation.messages().len() - 1;

    // -- Exec
    let tokens = request_tokens((None, None), conversation.messages(), Some((5_000, last)));

    // -- Check
    assert_eq!(
        tokens, 5_000,
        "the reported count stands when nothing follows the anchored entry"
    );
    Ok(())
}
