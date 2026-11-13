//! Anchor metering: the request the decision records and the tokens the next
//! decision reads back from it.

use super::super::*;
use super::*;

use rig::conversation::Conversation;
use rig_memory::{Anchor, MemoryStore, RequestStart, next_request_tokens};

use crate::hook::chain::in_place::InPlace;

/// WHEN the decision meters, THE decision SHALL record the request it metered and
/// the entry a completion's usage lands on, so the next decision can anchor.
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
        InPlace {
            spec: Some(&fx.spec),
            pending: &fx.pending,
        },
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

    // -- Check: the request is recorded, and the completion that settles anchors
    // on the prompt's entry — the store's two entries plus the prompt.
    let (anchor, sent) = fx.meter.metering();
    assert!(sent.is_some(), "the metered request must be recorded");
    assert!(
        anchor.is_none(),
        "no usage has been reported, so nothing is anchored yet"
    );
    fx.meter.record_usage(usage(1_000, 10));
    let (anchor, _) = fx.meter.metering();
    let anchor = anchor.ok_or("the recorded entry anchors the usage")?;
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

/// WHEN the recorded request still describes the conversation, THE metering
/// SHALL read the anchor's reported usage plus the estimate of what follows it.
///
/// The anchor holds while the request's fingerprint does — a check over the
/// entries' own allocations, so it is pinned here against one conversation and a
/// copy of it, which is the shape rig documents.
#[test]
fn metering_is_anchored_when_the_request_still_continues_the_anchor() -> Result<()> {
    // -- Setup & Fixtures
    let mut conversation = Conversation::new();
    conversation.extend([Message::user("older"), Message::assistant("reply")]);
    let sent = prepared(&conversation).start();
    let through = conversation.id(1).ok_or("the second entry has an id")?;
    let anchor = Anchor {
        usage: usage(5_000, 10),
        through,
    };
    let copy = conversation.clone();

    // -- Exec
    let tokens = next_request_tokens(Some(&anchor), Some(sent), &prepared(&copy));

    // -- Check
    assert_eq!(
        tokens, 5_010,
        "an anchor that still holds meters its reported usage with nothing after it"
    );
    Ok(())
}

/// WHEN the request is a fresh load of the same conversation, THE metering SHALL
/// fall back to `whole_tokens`: the entries are new allocations, so the
/// fingerprint the anchor was made under no longer holds.
#[test]
fn metering_is_whole_tokens_when_the_request_is_a_fresh_load() -> Result<()> {
    // -- Setup & Fixtures: a store holding the conversation, loaded twice.
    let store = MemoryStore::default();
    let mut conversation = rig_memory::load(&store, "conv-1")?;
    conversation.extend([Message::user("older"), Message::assistant("reply")]);
    rig_memory::save(&store, "conv-1", &mut conversation)?;
    let sent: RequestStart = prepared(&conversation).start();
    let through = conversation.id(1).ok_or("the second entry has an id")?;
    let anchor = Anchor {
        usage: usage(5_000, 10),
        through,
    };
    let reloaded = rig_memory::load(&store, "conv-1")?;

    // -- Exec
    let tokens = next_request_tokens(Some(&anchor), Some(sent), &prepared(&reloaded));

    // -- Check
    assert_eq!(
        tokens,
        prepared(&reloaded).whole_tokens(),
        "a request the anchor no longer describes is metered whole"
    );
    assert_ne!(
        tokens, 5_010,
        "the anchored usage must not be read across a fresh load"
    );
    Ok(())
}
