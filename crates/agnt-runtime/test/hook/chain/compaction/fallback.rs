//! The fallback the settle site owns: a patched call whose reply cannot be a
//! summary is replaced by one asked on its own, on the turn's own model.

use super::super::*;
use super::*;

use rig::completion::{CompletionResponse, Usage};
use rig::message::{AssistantContent, Origin};
use rig_memory::{SummaryCut, SummaryState};

use crate::hook::chain::compaction::settle_fallback;

// region:    --- Test Support

/// The completion answer a patched call returns: `text`, and no finish reason.
fn answer(text: &str) -> CompletionResponse {
    CompletionResponse::new(
        vec![AssistantContent::text(text)],
        Usage::new().input_tokens(10).output_tokens(5),
        Origin::new("openai", "openai", "gpt-4o"),
        serde_json::Value::Null,
    )
}

// endregion: --- Test Support

/// WHEN the patched call's reply fails validation, THEN THE settle site SHALL run
/// the side-call on the turn's own model and stash its summary for the executor
/// — the rejected verdict is replaced, so the second pass still opens on a
/// summarized conversation.
#[tokio::test]
async fn rejected_reply_is_replaced_by_a_side_call_on_the_turn_model() -> Result<()> {
    // -- Setup & Fixtures: a conversation with something to summarize, a plan the
    // decision stashed, and a reply that cannot be a summary.
    let fx = decide_fixture()?;
    seed_rig(
        &fx,
        &[
            Message::user("an older question"),
            Message::user("q".repeat(200_000)),
        ],
    )
    .await?;
    fx.pending.stash(
        SummaryCut {
            keep_from: None,
            state: SummaryState::default(),
        },
        CompactionSource::Auto,
        vec![],
    );
    fx.pending.settle(&answer(""));
    assert!(
        fx.pending.settled_rejected(),
        "an empty reply must be refused as a summary"
    );

    // -- Exec
    settle_fallback(
        "conv-1",
        &CompactionPath::new(
            fx.memory.as_ref(),
            &fx.compaction,
            &fx.meter,
            &Default::default(),
            &fx.model,
        ),
        &fx.pending,
        fx.bus.stream(),
        fx.bus.lifecycle(),
    )
    .await;

    // -- Check: the side-call's summary replaced the rejected verdict, and it ran
    // on the turn's model rather than the compactor's.
    let settled = fx
        .pending
        .take_settled()
        .ok_or("the side-call's summary must wait for the executor")?;
    assert_eq!(
        settled.summary(),
        Some("summary"),
        "the fallback must stash a usable summary"
    );
    assert_eq!(
        settled.rejection(),
        None,
        "the rejected verdict must be replaced, not kept beside it"
    );
    assert_eq!(
        fx.turn_model.requests().len(),
        1,
        "the fallback asks the turn's own model exactly once"
    );
    assert_eq!(
        fx.summary_model.requests().len(),
        0,
        "the fallback must not use the compactor's own model"
    );
    Ok(())
}

/// WHEN no reply was refused, THEN THE settle site SHALL run no side-call: the
/// turn proceeds with the verdict the reply earned.
#[tokio::test]
async fn settle_fallback_does_nothing_without_a_rejected_reply() -> Result<()> {
    // -- Setup & Fixtures: a conversation over the threshold, and a slot with no
    // verdict at all.
    let fx = decide_fixture()?;
    seed_rig(&fx, &[Message::user("q".repeat(200_000))]).await?;

    // -- Exec
    settle_fallback(
        "conv-1",
        &CompactionPath::new(
            fx.memory.as_ref(),
            &fx.compaction,
            &fx.meter,
            &Default::default(),
            &fx.model,
        ),
        &fx.pending,
        fx.bus.stream(),
        fx.bus.lifecycle(),
    )
    .await;

    // -- Check
    assert!(
        fx.pending.take_settled().is_none(),
        "a turn whose reply was never refused stashes no side-call"
    );
    assert_eq!(
        fx.turn_model.requests().len(),
        0,
        "no refusal means no side-call"
    );
    Ok(())
}
