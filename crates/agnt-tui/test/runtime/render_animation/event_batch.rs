use super::*;

use agnt_render::transcript::entry::TranscriptEntry;
use agnt_runtime::protocol::events::{CompactionSource, PermissionRecord, RequestId};
use agnt_runtime::session::tool_status::ToolStatus;

use crate::test_support::{compaction_summary_chunk, inference_chunk};

/// One streamed delta of a compaction summary, on the record stream.
fn compaction_chunk(source: CompactionSource, delta: &str, aggregated: &str) -> SessionRecord {
    compaction_summary_chunk(source, delta, aggregated)
}

/// The assistant entries' text, in transcript order.
fn assistant_texts(coordinator: &RuntimeCoordinator) -> Vec<String> {
    coordinator
        .state()
        .transcript
        .entries()
        .iter()
        .filter(|entry| {
            matches!(
                entry,
                TranscriptEntry::Message(message) if message.role == MessageRole::Assistant
            )
        })
        .map(entry_text)
        .collect()
}

/// The tool entries of the transcript, in order.
fn tool_entries(coordinator: &RuntimeCoordinator) -> Vec<&TranscriptEntry> {
    coordinator
        .state()
        .transcript
        .entries()
        .iter()
        .filter(|entry| matches!(entry, TranscriptEntry::Tool(_)))
        .collect()
}

/// A `read` call for the tool-ordering tests.
fn read_call() -> ToolCall {
    ToolCall::new(
        ToolName("read".to_string()),
        agnt_runtime::tools::handler::ToolSource::Unknown,
        "{}",
    )
}

/// A permission request for the coalescing-bypass test.
fn permission_requested(request_id: &str) -> SessionRecord {
    SessionRecord::Permission(PermissionRecord::Requested {
        request_id: RequestId::from_minted(request_id),
        call_id: agnt_runtime::protocol::events::ToolCallId::from_key(
            "nu\n{\"command\":\"echo hi\"}",
        ),
        tool: "nu(command=echo hi)".to_string(),
        source: "closure".to_string(),
        mode: Some("apply".to_string()),
        rule_identity: "nested:nu.command:*".to_string(),
        scope: "nested".to_string(),
        target_field: Some("command".to_string()),
        pattern: "*".to_string(),
        summary: "→ {\"command\":\"echo hi\"}".to_string(),
        shows_preview: true,
        preview: None,
    })
}

/// A burst of consecutive chunks folds only its last member: the transcript
/// holds one streamed entry with that chunk's aggregated text, and the fold
/// reports a change and marks the render needed.
#[test]
fn test_coordinator_reduce_narration_batch_coalesces_consecutive_chunks() -> Result<()> {
    // -- Setup & Fixtures: a realistic incremental stream in one batch.
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));
    coordinator.set_render_needed(false);
    let batch = [
        ("a", "a"),
        ("b", "ab"),
        ("c", "abc"),
        ("d", "abcd"),
        ("e", "abcde"),
    ]
    .into_iter()
    .map(|(delta, aggregated)| inference_chunk(delta, aggregated));

    // -- Exec
    let changed = coordinator.reduce_narration_batch(batch);

    // -- Check
    assert!(changed, "a chunk fold changes state");
    assert!(
        coordinator.render_needed(),
        "a change marks the render needed"
    );
    let texts = assistant_texts(&coordinator);
    assert_eq!(texts, vec!["abcde".to_string()], "got {texts:?}");
    Ok(())
}

/// Coalescing changes the fold count, not the outcome: the burst's final
/// state equals the state of folding only its last chunk.
#[test]
fn test_coordinator_reduce_narration_batch_matches_folding_only_the_last_chunk() -> Result<()> {
    // -- Setup & Fixtures: the same stream as a burst and as its tail alone.
    let mut burst = RuntimeCoordinator::new(120, 40, Some(false));
    let mut tail = RuntimeCoordinator::new(120, 40, Some(false));

    // -- Exec
    let burst_changed = burst.reduce_narration_batch([
        inference_chunk("a", "a"),
        inference_chunk("b", "ab"),
        inference_chunk("c", "abc"),
    ]);
    let tail_changed = tail.reduce_narration_batch([inference_chunk("c", "abc")]);

    // -- Check
    assert!(burst_changed && tail_changed);
    let burst_texts = assistant_texts(&burst);
    assert_eq!(burst_texts, assistant_texts(&tail), "got {burst_texts:?}");
    assert_eq!(burst_texts, vec!["abc".to_string()]);
    Ok(())
}

/// The record batch coalesces its own chunk kind: a burst of compaction
/// summary chunks leaves only the newest aggregated text.
#[test]
fn test_coordinator_reduce_record_batch_coalesces_compaction_summary_chunks() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));

    // -- Exec
    let changed = coordinator.reduce_record_batch([
        compaction_chunk(CompactionSource::Auto, "s1", "summary one"),
        compaction_chunk(CompactionSource::Auto, "s2", "summary two"),
    ]);

    // -- Check: one assistant entry, holding the newest aggregated text.
    assert!(changed);
    let texts = assistant_texts(&coordinator);
    assert_eq!(texts, vec!["summary two".to_string()], "got {texts:?}");
    Ok(())
}

/// A pending chunk flushes before any other event, so a non-chunk event that
/// closes the stream still lands after the chunk it followed.
#[test]
fn test_coordinator_reduce_narration_batch_flushes_a_pending_chunk_before_a_non_chunk_event()
-> Result<()> {
    // -- Setup & Fixtures: the turn error closes the streamed block, so the
    // order of the two chunks is visible in the entry count.
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));

    // -- Exec
    let changed = coordinator.reduce_narration_batch([
        inference_chunk("a", "a"),
        system_message(
            agnt_runtime::protocol::events::NoticeKind::TurnError,
            "boom",
        ),
        inference_chunk("b", "b"),
    ]);

    // -- Check: the first chunk is not dropped by the later non-chunk event.
    assert!(changed);
    let texts = assistant_texts(&coordinator);
    assert_eq!(
        texts,
        vec!["a".to_string(), "b".to_string()],
        "got {texts:?}"
    );
    Ok(())
}

/// Non-chunk events reduce in arrival order: a start/completion pair folds
/// onto one tool entry instead of pushing two.
#[test]
fn test_coordinator_reduce_record_batch_reduces_non_chunk_events_in_order() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));
    let call = read_call();

    // -- Exec
    let changed = coordinator.reduce_record_batch([
        tool_started(call.clone()),
        tool_completed(
            call,
            ToolOutcome {
                status: ToolStatus::Success,
                payload: None,
                text: "{}".to_string(),
                message: None,
                kind: None,
            },
        ),
    ]);

    // -- Check: the completion closes the entry the start pushed.
    assert!(changed);
    let tools = tool_entries(&coordinator);
    assert_eq!(
        tools.len(),
        1,
        "the completion must land on the started entry"
    );
    assert!(matches!(
        tools[0],
        TranscriptEntry::Tool(tool) if tool.status == ItemStatus::Done
    ));
    Ok(())
}

/// A permission request is never coalesced: it folds after any pending chunk
/// and opens the card.
#[test]
fn test_coordinator_reduce_record_batch_never_coalesces_permission_requests() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));
    coordinator.set_render_needed(false);

    // -- Exec
    let changed = coordinator.reduce_record_batch([
        compaction_chunk(CompactionSource::Auto, "hello", "hello"),
        permission_requested("ask-0000000000000002"),
    ]);

    // -- Check: the pending chunk flushed, then the prompt opened.
    assert!(changed);
    assert!(coordinator.render_needed());
    assert_eq!(assistant_texts(&coordinator), vec!["hello".to_string()]);
    assert!(coordinator.state().permission.has_prompt());
    Ok(())
}

/// An empty batch returns `false` and mutates nothing, including the render
/// flag.
#[test]
fn test_coordinator_reduce_record_batch_empty_batch_is_a_no_op() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));
    coordinator.set_render_needed(false);
    let empty: [SessionRecord; 0] = [];

    // -- Exec
    let changed = coordinator.reduce_record_batch(empty);

    // -- Check
    assert!(!changed);
    assert!(!coordinator.render_needed(), "an empty batch marks nothing");
    assert!(coordinator.state().transcript.entries().is_empty());
    Ok(())
}

/// A batch whose folds all report no change returns `false` and leaves the
/// render flag alone.
#[test]
fn test_coordinator_reduce_record_batch_unchanged_folds_do_not_mark_the_render() -> Result<()> {
    // -- Setup & Fixtures: a compaction request is a command to the loop owner,
    // not a row, so the fold drops it.
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));
    coordinator.set_render_needed(false);

    // -- Exec
    let changed = coordinator.reduce_record_batch([SessionRecord::Compaction(
        agnt_runtime::protocol::events::CompactionRecord::Requested {
            source: CompactionSource::Auto,
        },
    )]);

    // -- Check
    assert!(!changed);
    assert!(!coordinator.render_needed());
    Ok(())
}

/// One event through the batch path matches one event through the single-fold
/// path: same store state, same changed flag, plus the render mark.
#[test]
fn test_coordinator_reduce_narration_batch_single_event_matches_the_single_fold_path() -> Result<()>
{
    // -- Setup & Fixtures
    let mut batched = RuntimeCoordinator::new(120, 40, Some(false));
    batched.set_render_needed(false);
    let mut direct = RuntimeCoordinator::new(120, 40, Some(false));
    direct.set_render_needed(false);

    // -- Exec
    let batched_changed = batched.reduce_narration_batch([inference_chunk("solo", "solo")]);
    let direct_changed = direct.fold_narration(&inference_chunk("solo", "solo"));

    // -- Check
    assert_eq!(batched_changed, direct_changed);
    assert_eq!(assistant_texts(&batched), assistant_texts(&direct));
    assert_eq!(
        batched.state().transcript.assistant_stream_start,
        direct.state().transcript.assistant_stream_start
    );
    assert!(batched.render_needed(), "the batch marks its own render");
    Ok(())
}
