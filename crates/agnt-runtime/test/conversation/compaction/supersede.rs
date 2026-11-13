//! The between-runs splice: the patched call's reply becomes the stored
//! conversation's summary, between pass one's save and pass two's load.
//!
//! The tests pin the folded store state after the splice: the summary entry by
//! `summary_author`, the kept messages after it, and no assistant turn left
//! where the model's internal call wrote its reply.

use std::sync::Arc;

use rig::completion::{CompletionResponse, Usage};
use rig::message::{AssistantContent, Origin};
use rig_memory::{Store, SummaryCut, SummaryState, summary_author};

use crate::bus::Bus;
use crate::conversation::compaction::supersede::{Landed, land};
use crate::hook::chain::PendingSummary;
use crate::session::RigStoreConfig;
use crate::types::{AssistantMessage, CallId, Message, ToolCall, ToolFunction, ToolName};
use agnt_kernel::bus::LifecycleEvent;

const CONV: &str = "conv-1";
const SUMMARY: &str = "the older turns, summarized";

// region:    --- Test Support

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// An in-memory rig store: a memory store resolves no path, so no ambient cache
/// directory is touched.
async fn memory_store() -> Result<Arc<dyn Store>> {
    Ok(RigStoreConfig::in_memory().open(CONV).await?)
}

/// Seed the store with a conversation as pass one leaves it: the older turns,
/// the turn's prompt, and the reply of the call the decision patched.
async fn seeded(store: &Arc<dyn Store>, reply: &str) -> Result<()> {
    let mut conversation = rig_memory::load(&**store, CONV)?;
    conversation.extend([
        Message::user("an older question"),
        Message::assistant("an older answer"),
        Message::user("the turn's prompt"),
        Message::assistant(reply),
    ]);
    rig_memory::save(&**store, CONV, &mut conversation)?;
    Ok(())
}

/// The tool result parts of `message`, in order.
fn tool_results(message: &Message) -> Vec<&crate::types::ToolResult> {
    let Message::User { content } = message else {
        return Vec::new();
    };
    content
        .iter()
        .filter_map(|item| match item {
            crate::types::UserContent::ToolResult(result) => Some(result),
            _ => None,
        })
        .collect()
}

/// Seed the store with a reply that called a tool: the older turns, the turn's
/// prompt, the textless reply carrying one tool call, the result answering that
/// call, and the assistant turn the run ended with.
async fn seeded_with_tool_call(store: &Arc<dyn Store>) -> Result<()> {
    let name = ToolName::new("tool").map_err(|_| "tool is a non-empty tool name")?;
    let call = CallId::from_wire("tc-1");
    let call_part = AssistantContent::ToolCall(ToolCall::new(
        call.clone(),
        ToolFunction::new(name.clone(), serde_json::Value::Null),
    ));
    let mut conversation = rig_memory::load(&**store, CONV)?;
    conversation.extend([
        Message::user("an older question"),
        Message::assistant("an older answer"),
        Message::user("the turn's prompt"),
        Message::Assistant(AssistantMessage::new(vec![call_part])),
        Message::tool_result(call.clone(), name, "the tool answered"),
        Message::assistant("the real answer"),
    ]);
    rig_memory::save(&**store, CONV, &mut conversation)?;
    Ok(())
}

/// The cut a decision stashes: the prompt is the first message kept, so the
/// older turns are what the summary replaces.
async fn cut(store: &Arc<dyn Store>) -> Result<SummaryCut> {
    let conversation = rig_memory::load(&**store, CONV)?;
    let at = conversation
        .messages()
        .iter()
        .position(|message| user_text(message) == "the turn's prompt")
        .ok_or("the seeded conversation holds the turn's prompt")?;
    Ok(SummaryCut {
        keep_from: conversation.id(at),
        state: SummaryState::default(),
    })
}

/// The completion answer the patched call returns: one text choice and no stop
/// reason, so `summary_text` accepts it.
fn answer(text: &str) -> CompletionResponse {
    CompletionResponse::new(
        vec![AssistantContent::text(text)],
        Usage::new().input_tokens(10).output_tokens(5),
        Origin::new("openai", "openai", "gpt-4o"),
        serde_json::Value::Null,
    )
}

/// The reply the hook's settle validated, taken from the slot it wrote it to.
fn settled(pending: &PendingSummary) -> Result<crate::hook::chain::Supersede> {
    pending
        .take_settled()
        .ok_or_else(|| "the patched call's verdict must be stashed for the executor".into())
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
            AssistantContent::Text(text) => Some(text.text.clone()),
            _ => None,
        })
        .collect()
}

// endregion: --- Test Support

/// WHEN the summary reply validates, THE between-runs splice SHALL leave the
/// folded store conversation leading with the summary under `summary_author`,
/// keeping the turn's prompt, and holding NO assistant entry for the reply.
#[tokio::test]
async fn validated_reply_lands_as_the_stored_summary() -> Result<()> {
    // -- Setup & Fixtures: pass one's exchange, already saved.
    let store = memory_store().await?;
    seeded(&store, SUMMARY).await?;
    let bus = Bus::default();
    let mut rx = bus.lifecycle().subscribe();
    let pending_summary = PendingSummary::default();
    let answer = answer(SUMMARY);

    // -- Exec: the decision stashes, the settle validates, the executor splices.
    pending_summary.stash(
        cut(&store).await?,
        agnt_kernel::CompactionSource::Auto,
        vec![],
    );
    pending_summary.settle(&answer);
    let landed = land(
        settled(&pending_summary)?,
        Arc::clone(&store),
        CONV.to_string(),
        bus.stream(),
        bus.lifecycle(),
    )
    .await;

    // -- Check: the summary leads, the prompt is kept, the reply is gone.
    assert!(
        matches!(&landed, Landed::Summary(text) if text == SUMMARY),
        "the validated reply must land as the summary, got {landed:?}"
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
        "the leading entry must carry the validated summary, got {:?}",
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
            .all(|message| assistant_text(message) != SUMMARY),
        "the internal call must not read back as an assistant turn"
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

/// WHEN the reply fails `summary_text` validation, THEN THE splice SHALL remove
/// only the reply entry — no summary inserted — and SHALL publish `Failed`.
#[tokio::test]
async fn rejected_reply_removes_only_the_reply_entry() -> Result<()> {
    // -- Setup & Fixtures: an empty answer, which `summary_text` refuses.
    let store = memory_store().await?;
    seeded(&store, "").await?;
    let before = rig_memory::load(&*store, CONV)?.messages().len();
    let bus = Bus::default();
    let mut rx = bus.lifecycle().subscribe();
    let pending_summary = PendingSummary::default();
    let answer = answer("");

    // -- Exec
    pending_summary.stash(
        cut(&store).await?,
        agnt_kernel::CompactionSource::Auto,
        vec![],
    );
    pending_summary.settle(&answer);
    let landed = land(
        settled(&pending_summary)?,
        Arc::clone(&store),
        CONV.to_string(),
        bus.stream(),
        bus.lifecycle(),
    )
    .await;

    // -- Check: no summary, one entry fewer, and a Failed row.
    assert!(
        matches!(&landed, Landed::Rejected(_)),
        "an empty answer must not land as a summary, got {landed:?}"
    );
    let conversation = rig_memory::load(&*store, CONV)?;
    assert!(
        conversation
            .entries()
            .all(|entry| entry.author != Some(&summary_author())),
        "a rejected reply must insert no summary"
    );
    assert_eq!(
        conversation.messages().len(),
        before - 1,
        "a rejected reply must remove exactly its own entry"
    );
    let failed = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .map_err(|_| "should receive CompactionFailed within timeout")?
        .map_err(|_| "should receive CompactionFailed")?;
    assert!(
        matches!(&failed, LifecycleEvent::CompactionFailed { .. }),
        "an unusable summary must be announced, got {failed:?}"
    );
    Ok(())
}

/// WHEN the rejected reply carried a tool call and the store holds the result
/// answering it, THEN THE splice SHALL remove the result with the reply, keep
/// the run's real answer and the earlier turns, and insert no summary.
#[tokio::test]
async fn rejected_reply_takes_its_paired_tool_records() -> Result<()> {
    // -- Setup & Fixtures: an empty answer over a reply that called a tool.
    let store = memory_store().await?;
    seeded_with_tool_call(&store).await?;
    let bus = Bus::default();
    let pending_summary = PendingSummary::default();
    let answer = answer("");

    // -- Exec
    pending_summary.stash(
        cut(&store).await?,
        agnt_kernel::CompactionSource::Auto,
        vec![],
    );
    pending_summary.settle(&answer);
    let landed = land(
        settled(&pending_summary)?,
        Arc::clone(&store),
        CONV.to_string(),
        bus.stream(),
        bus.lifecycle(),
    )
    .await;

    // -- Check: the call and its result are gone together, nothing else is.
    assert!(
        matches!(&landed, Landed::Rejected(_)),
        "an empty answer must not land as a summary, got {landed:?}"
    );
    let conversation = rig_memory::load(&*store, CONV)?;
    let unpaired = conversation.validate();
    assert!(
        unpaired.is_ok(),
        "the folded store must pair every tool call with its result, got {unpaired:?}"
    );
    assert!(
        conversation
            .messages()
            .iter()
            .all(|message| tool_results(message).is_empty()),
        "a tool result must not outlive the call it answers"
    );
    assert!(
        conversation
            .messages()
            .iter()
            .any(|message| assistant_text(message) == "the real answer"),
        "the cleanup must keep the run's real answer"
    );
    assert!(
        conversation
            .messages()
            .iter()
            .any(|message| user_text(message) == "the turn's prompt"),
        "the cleanup must keep the turn's prompt"
    );
    assert!(
        conversation
            .messages()
            .iter()
            .any(|message| assistant_text(message) == "an older answer"),
        "the cleanup must keep the older turns"
    );
    assert!(
        conversation
            .entries()
            .all(|entry| entry.author != Some(&summary_author())),
        "a rejected reply must insert no summary"
    );
    Ok(())
}

/// WHEN a turn is patched, THE decision SHALL let the turn's request patch
/// once: a second pass over the same turn takes no second attempt.
#[tokio::test]
async fn one_attempt_per_turn_patch() -> Result<()> {
    // -- Setup & Fixtures: a slot that already stashed a plan.
    let store = memory_store().await?;
    seeded(&store, SUMMARY).await?;
    let pending_summary = PendingSummary::default();
    pending_summary.stash(
        cut(&store).await?,
        agnt_kernel::CompactionSource::Auto,
        vec![],
    );

    // -- Exec & Check: the attempt is spent until the turn-end sweep.
    assert!(
        pending_summary.attempted(),
        "a stashed plan spends the turn's one attempt"
    );
    assert!(
        pending_summary.awaiting_reply().is_some(),
        "a stashed plan is waiting for the reply its deltas belong to"
    );
    pending_summary.clear();
    assert!(
        !pending_summary.attempted() && pending_summary.awaiting_reply().is_none(),
        "the turn-end sweep must leave nothing for the next turn"
    );
    Ok(())
}
