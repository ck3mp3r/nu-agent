use super::*;

use agnt_core::session::tool_status::ToolStatus;
use agnt_core::transcript::entry::TranscriptEntry;

/// Build a chunk with the producer's two facts kept apart: `delta` is the new
/// suffix, `aggregated` the accumulated text.
fn chunk(delta: &str, aggregated: &str) -> SystemEvent {
    SystemEvent::InferenceChunk {
        delta: delta.to_string(),
        aggregated: aggregated.to_string(),
    }
}

/// Build one streamed delta of a compaction summary.
fn compaction_chunk(source: &str, delta: &str, aggregated: &str) -> SystemEvent {
    SystemEvent::CompactionSummaryChunk {
        source: source.to_string(),
        delta: delta.to_string(),
        aggregated: aggregated.to_string(),
    }
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
    ToolCall::new(ToolName("read".to_string()), "user", "{}")
}

/// A permission request for the coalescing-bypass test.
fn permission_requested(request_id: &str) -> SystemEvent {
    SystemEvent::PermissionRequested {
        request_id: RequestId::new(request_id),
        context: PermissionRequestContext {
            tool: "nu(command=echo hi)".to_string(),
            tool_key: "nu\n{\"command\":\"echo hi\"}".to_string(),
            source: "closure".to_string(),
            mode: Some("apply".to_string()),
            matched_rule_identity: "nested:nu.command:*".to_string(),
            scope: "nested".to_string(),
            target_field: Some("command".to_string()),
            pattern: "*".to_string(),
            summary: "→ {\"command\":\"echo hi\"}".to_string(),
            pre_authorize_display: None,
        },
    }
}

/// A burst of consecutive chunks folds only its last member: the transcript
/// holds one streamed entry with that chunk's aggregated text, and the fold
/// reports a change and marks the render needed.
#[test]
fn test_coordinator_reduce_event_batch_coalesces_consecutive_chunks() -> Result<()> {
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
    .map(|(delta, aggregated)| chunk(delta, aggregated));

    // -- Exec
    let changed = coordinator.reduce_event_batch(batch);

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
fn test_coordinator_reduce_event_batch_matches_folding_only_the_last_chunk() -> Result<()> {
    // -- Setup & Fixtures: the same stream as a burst and as its tail alone.
    let mut burst = RuntimeCoordinator::new(120, 40, Some(false));
    let mut tail = RuntimeCoordinator::new(120, 40, Some(false));

    // -- Exec
    let burst_changed =
        burst.reduce_event_batch([chunk("a", "a"), chunk("b", "ab"), chunk("c", "abc")]);
    let tail_changed = tail.reduce_event_batch([chunk("c", "abc")]);

    // -- Check
    assert!(burst_changed && tail_changed);
    let burst_texts = assistant_texts(&burst);
    assert_eq!(burst_texts, assistant_texts(&tail), "got {burst_texts:?}");
    assert_eq!(burst_texts, vec!["abc".to_string()]);
    Ok(())
}

/// Each chunk kind coalesces only against its own kind, and a pending chunk
/// flushes before the next non-chunk event: the inference chunk lands first,
/// then the compaction summary.
#[test]
fn test_coordinator_reduce_event_batch_coalesces_each_chunk_kind_separately() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));

    // -- Exec
    let changed = coordinator.reduce_event_batch([
        chunk("a", "a"),
        compaction_chunk("auto", "s1", "summary one"),
        compaction_chunk("auto", "s2", "summary two"),
    ]);

    // -- Check: one entry per kind, each holding its newest aggregated text.
    assert!(changed);
    let texts = assistant_texts(&coordinator);
    assert_eq!(
        texts,
        vec!["a".to_string(), "summary two".to_string()],
        "got {texts:?}"
    );
    Ok(())
}

/// A pending chunk flushes before any other event, so a non-chunk event that
/// closes the stream still lands after the chunk it followed.
#[test]
fn test_coordinator_reduce_event_batch_flushes_a_pending_chunk_before_a_non_chunk_event()
-> Result<()> {
    // -- Setup & Fixtures: `InferenceStarted` resets the stream cursor, so the
    // order of the two chunks is visible in the entry count.
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));

    // -- Exec
    let changed = coordinator.reduce_event_batch([
        chunk("a", "a"),
        SystemEvent::InferenceStarted,
        chunk("b", "b"),
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
fn test_coordinator_reduce_event_batch_reduces_non_chunk_events_in_order() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));
    let call = read_call();

    // -- Exec
    let changed = coordinator.reduce_event_batch([
        SystemEvent::ToolStarted { call: call.clone() },
        SystemEvent::ToolCompleted {
            call,
            outcome: ToolOutcome {
                success: true,
                status: ToolStatus::Success,
                result: "{}".to_string(),
                display: None,
                error_kind: None,
                message: None,
            },
        },
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
fn test_coordinator_reduce_event_batch_never_coalesces_permission_requests() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));
    coordinator.set_render_needed(false);

    // -- Exec
    let changed = coordinator.reduce_event_batch([
        chunk("hello", "hello"),
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
fn test_coordinator_reduce_event_batch_empty_batch_is_a_no_op() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));
    coordinator.set_render_needed(false);
    let empty: [SystemEvent; 0] = [];

    // -- Exec
    let changed = coordinator.reduce_event_batch(empty);

    // -- Check
    assert!(!changed);
    assert!(!coordinator.render_needed(), "an empty batch marks nothing");
    assert!(coordinator.state().transcript.entries().is_empty());
    Ok(())
}

/// A batch whose folds all report no change returns `false` and leaves the
/// render flag alone.
#[test]
fn test_coordinator_reduce_event_batch_unchanged_folds_do_not_mark_the_render() -> Result<()> {
    // -- Setup & Fixtures: `CompactionRequested` is an echo the reducer drops.
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));
    coordinator.set_render_needed(false);

    // -- Exec
    let changed = coordinator.reduce_event_batch([SystemEvent::CompactionRequested {
        source: agnt_core::protocol::events::CompactionSource::Auto,
    }]);

    // -- Check
    assert!(!changed);
    assert!(!coordinator.render_needed());
    Ok(())
}

/// One event through the batch path matches one event through the single-fold
/// path: same store state, same changed flag, plus the render mark.
#[test]
fn test_coordinator_reduce_event_batch_single_event_matches_the_single_fold_path() -> Result<()> {
    // -- Setup & Fixtures
    let mut batched = RuntimeCoordinator::new(120, 40, Some(false));
    batched.set_render_needed(false);
    let mut direct = RuntimeCoordinator::new(120, 40, Some(false));
    direct.set_render_needed(false);

    // -- Exec
    let batched_changed = batched.reduce_event_batch([chunk("solo", "solo")]);
    let direct_changed = direct.reduce_system_event(chunk("solo", "solo"));

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
