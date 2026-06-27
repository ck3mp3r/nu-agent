use super::helpers::{has_tool_call, has_tool_result};
use super::*;
use crate::session::{JournalConversationMemory, StoreEntry, extract_llm_context};
use crate::types::{
    AssistantContent, Message, Text, ToolCall, ToolFunction, ToolResult, ToolResultContent,
    UserContent,
};
use rig::memory::ConversationMemory;
use rig::one_or_many::OneOrMany;
use serde_json::json;
use tempfile::TempDir;

/// Create N alternating user/assistant messages for testing.
///
/// The last message is always an assistant so `trim_trailing_user` in
/// `repair_messages` leaves the count unchanged.  If `n` is odd the count
/// is silently rounded up to `n + 1`.
fn make_test_messages(n: usize) -> Vec<Message> {
    let count = if n.is_multiple_of(2) { n } else { n + 1 };
    (0..count)
        .map(|i| {
            if i.is_multiple_of(2) {
                Message::user(format!("msg{}", i))
            } else {
                Message::assistant(format!("msg{}", i))
            }
        })
        .collect()
}

/// Helper: build an Assistant message containing a single ToolCall.
fn make_tool_call_message(call_id: &str, tool_name: &str) -> Message {
    Message::Assistant {
        id: None,
        content: OneOrMany::one(AssistantContent::ToolCall(ToolCall {
            id: call_id.to_string(),
            call_id: None,
            signature: None,
            additional_params: None,
            function: ToolFunction {
                name: tool_name.to_string(),
                arguments: json!({}),
            },
        })),
    }
}

/// Helper: build a User message containing a single ToolResult.
fn make_tool_result_message(call_id: &str, result_text: &str) -> Message {
    Message::User {
        content: OneOrMany::one(UserContent::ToolResult(ToolResult {
            id: call_id.to_string(),
            call_id: None,
            content: OneOrMany::one(ToolResultContent::Text(Text {
                text: result_text.to_string(),
                additional_params: None,
            })),
        })),
    }
}

#[test]
fn compact_splits_at_keep_recent() {
    // RED: Test that compaction loads from memory, splits correctly, and stores back
    let temp_dir = TempDir::new().unwrap();
    let memory = JournalConversationMemory::new(temp_dir.path().to_path_buf());
    let session_id = "test_session";

    // Setup: 10 alternating messages in memory (ends with assistant — survives repair)
    let messages = make_test_messages(10);

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });

    let keep_recent = 3;
    let config = CompactionParams {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingSummary,
        keep_recent,
        token_budget: None,
    };

    // Mock summarizer that formats old messages
    let summarizer = |old_messages: &[Message]| {
        let count = old_messages.len();
        async move { Ok((format!("Summary of {} messages", count), None)) }
    };

    let outcome = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { super::compact(session_id, &config, &memory, summarizer).await })
        .unwrap();

    // Verify: 7 messages summarized, 3 kept recent
    assert_eq!(outcome.summarized_count, 7);
    assert_eq!(outcome.kept_recent_count, 3);
    assert_eq!(outcome.summary_text, "Summary of 7 messages");

    // Verify: memory contains summary + 3 recent messages = 4 total
    let final_messages = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { memory.load(session_id).await.unwrap() });

    assert_eq!(final_messages.len(), 4);

    // First message should be system with summary
    match &final_messages[0] {
        Message::System { content } => {
            assert_eq!(content, "Summary of 7 messages");
        }
        _ => panic!("Expected first message to be system"),
    }

    // Remaining 3 should be the kept recent messages (mixed user/assistant)
    for msg in final_messages.iter().skip(1) {
        assert!(
            matches!(msg, Message::User { .. } | Message::Assistant { .. }),
            "Expected user or assistant message, got: {:?}",
            msg
        );
    }
}

#[test]
fn compact_persists_to_store() {
    // RED: Test that compacted messages are persisted to ConversationStore
    let temp_dir = TempDir::new().unwrap();
    let memory = JournalConversationMemory::new(temp_dir.path().to_path_buf());
    let session_id = "test_persist";

    // Setup: 6 alternating messages (make_test_messages(5) rounds to 6 for a clean split)
    let messages = make_test_messages(6);

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });

    let config = CompactionParams {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingSummary,
        keep_recent: 2,
        token_budget: None,
    };

    let summarizer = |_: &[Message]| async move { Ok(("Summary".to_string(), None)) };

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { super::compact(session_id, &config, &memory, summarizer).await })
        .unwrap();

    // Verify: store contains all original messages + compaction marker + re-appended kept
    let (stored, _) = memory.load_all(session_id).unwrap();
    assert_eq!(stored.len(), 9); // 6 original + 1 marker + 2 kept

    // Marker should be at index 6
    match &stored[6] {
        StoreEntry::Marker(marker) => {
            assert_eq!(marker.summary, "Summary");
            assert_eq!(marker.kept_recent_count, 2);
        }
        _ => panic!("Expected compaction marker at index 6"),
    }

    // Last 2 entries should be re-appended kept messages
    assert!(matches!(&stored[7], StoreEntry::Message(_)));
    assert!(matches!(&stored[8], StoreEntry::Message(_)));
}

#[test]
fn compact_handles_insufficient_messages() {
    // RED: Test no-op when messages <= keep_recent
    let temp_dir = TempDir::new().unwrap();
    let memory = JournalConversationMemory::new(temp_dir.path().to_path_buf());
    let session_id = "test_noop";

    // Setup: only 2 alternating messages, keep_recent = 3
    let messages = vec![Message::user("A"), Message::assistant("B")];

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });

    let config = CompactionParams {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingSummary,
        keep_recent: 3,
        token_budget: None,
    };

    let summarizer = |_: &[Message]| async move { Ok(("Summary".to_string(), None)) };

    let outcome = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { super::compact(session_id, &config, &memory, summarizer).await })
        .unwrap();

    // Should be no-op
    assert_eq!(outcome.summarized_count, 0);
    assert_eq!(outcome.kept_recent_count, 2);
    assert_eq!(outcome.summary_text, String::new());

    // Memory unchanged
    let final_messages = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { memory.load(session_id).await.unwrap() });

    assert_eq!(final_messages.len(), 2);
}

#[test]
fn compact_clears_before_append() {
    // RED: Test that memory is cleared before appending compacted messages
    let temp_dir = TempDir::new().unwrap();
    let memory = JournalConversationMemory::new(temp_dir.path().to_path_buf());
    let session_id = "test_clear";

    let messages = make_test_messages(6);

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });

    let config = CompactionParams {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingSummary,
        keep_recent: 2,
        token_budget: None,
    };

    let summarizer = |old: &[Message]| {
        let count = old.len();
        async move { Ok((format!("Summarized {}", count), None)) }
    };

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { super::compact(session_id, &config, &memory, summarizer).await })
        .unwrap();

    // Memory should contain exactly 3 messages (summary + 2 recent), not 6 + 3
    let final_messages = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { memory.load(session_id).await.unwrap() });

    assert_eq!(final_messages.len(), 3);
}

#[test]
fn compact_with_async_summarizer_does_not_panic() {
    // This test mimics the production call chain where compact() is called
    // inside a runtime.block_on(), and the summarizer itself is async.
    // Before the async fix, this would panic with "Cannot start a runtime
    // from within a runtime".
    let temp_dir = TempDir::new().unwrap();
    let memory = JournalConversationMemory::new(temp_dir.path().to_path_buf());
    let session_id = "test_async_no_panic";

    // 6 alternating messages (make_test_messages(5) rounds to 6; ends with assistant)
    let messages = make_test_messages(6);

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        memory.append(session_id, messages).await.unwrap();
    });

    let config = CompactionParams {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingSummary,
        keep_recent: 2,
        token_budget: None,
    };

    // Async summarizer that actually awaits something — this is the key.
    // A sync closure wrapped in async wouldn't trigger the bug.
    let summarizer = |old: &[Message]| {
        let count = old.len();
        async move {
            // Yield to the runtime — this ensures we're truly async
            tokio::task::yield_now().await;
            Ok((format!("Summarized {} messages", count), None))
        }
    };

    // This is the production pattern: block_on wrapping an async call
    // that internally awaits the summarizer
    let outcome = rt
        .block_on(async { super::compact(session_id, &config, &memory, summarizer).await })
        .unwrap();

    assert_eq!(outcome.summarized_count, 4); // 6 messages - 2 kept = 4 summarized
    assert_eq!(outcome.kept_recent_count, 2);
}

#[test]
fn compact_does_not_split_tool_call_result_pair() {
    // Integration test: 11 messages where TC/TR pair is at indices 6-7.
    // Naive split_index = 11 - 4 = 7, which would cut between TC (index 6) and TR (index 7).
    // Safe split should move back to index 6, keeping both in the recent window.
    let temp_dir = TempDir::new().unwrap();
    let memory = JournalConversationMemory::new(temp_dir.path().to_path_buf());
    let session_id = "test_tool_pair_split";

    // Starts with assistant, ends with assistant, no consecutive same-role messages.
    // TC is at index 6 (preceded by user at 5 — no consecutive-assistant issue).
    // TR is at index 7. Split at naive_index=7 would put TC in old, TR in recent (unsafe).
    let messages: Vec<Message> = vec![
        Message::assistant("a0"),
        Message::user("u1"),
        Message::assistant("a2"),
        Message::user("u3"),
        Message::assistant("a4"),
        Message::user("u5"),
        make_tool_call_message("tc1", "read_file"), // index 6: assistant TC
        make_tool_result_message("tc1", "file contents"), // index 7: user TR
        Message::assistant("a8"),
        Message::user("u9"),
        Message::assistant("a10"),
    ];

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });

    let config = CompactionParams {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingSummary,
        keep_recent: 4,
        token_budget: None,
    };

    let summarizer = |old: &[Message]| {
        let count = old.len();
        async move { Ok((format!("Summary of {} messages", count), None)) }
    };

    let outcome = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { super::compact(session_id, &config, &memory, summarizer).await })
        .unwrap();

    // Naive split at 7 → safe split at 6 → 6 summarized, 5 kept
    assert_eq!(outcome.summarized_count, 6);
    assert_eq!(outcome.kept_recent_count, 5);

    // Verify memory: summary + 5 recent = 6 messages
    let final_messages = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { memory.load(session_id).await.unwrap() });

    assert_eq!(final_messages.len(), 6);

    // First is the system summary
    match &final_messages[0] {
        Message::System { content } => {
            assert_eq!(content, "Summary of 6 messages");
        }
        _ => panic!("Expected system message"),
    }

    // The tool call pair (indices 1-2 in compacted) must be intact
    assert!(has_tool_call(&final_messages[1]));
    assert!(has_tool_result(&final_messages[2]));
}

#[test]
fn compact_store_written_before_memory() {
    // Verify store has compacted data after compact succeeds.
    // With append-only operations, store.append_marker() is the durable commit
    // point that happens BEFORE memory.clear()/memory.append(). This test
    // confirms the store can independently serve as a recovery source.
    let temp_dir = TempDir::new().unwrap();
    let memory = JournalConversationMemory::new(temp_dir.path().to_path_buf());
    let session_id = "test_store_first";

    // 8 alternating messages (ends with assistant — survives repair unchanged)
    let messages = make_test_messages(8);

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });

    let config = CompactionParams {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingSummary,
        keep_recent: 3,
        token_budget: None,
    };

    let summarizer = |old: &[Message]| {
        let count = old.len();
        async move { Ok((format!("Summary of {} messages", count), None)) }
    };

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { super::compact(session_id, &config, &memory, summarizer).await })
        .unwrap();

    // Store must have all original messages + compaction marker + re-appended kept
    let (stored, _) = memory.load_all(session_id).unwrap();
    assert_eq!(stored.len(), 12); // 8 original + 1 marker + 3 kept

    // Marker should be at index 8
    match &stored[8] {
        StoreEntry::Marker(marker) => {
            assert_eq!(marker.summary, "Summary of 5 messages");
            assert_eq!(marker.kept_recent_count, 3);
        }
        _ => panic!("Expected compaction marker at index 8"),
    }
}

#[test]
fn compact_successful_produces_correct_state() {
    // After a successful compact, memory has LLM context (summary + recent)
    // and store has full append-only history (all messages + marker).
    let temp_dir = TempDir::new().unwrap();
    let memory = JournalConversationMemory::new(temp_dir.path().to_path_buf());
    let session_id = "test_consistent_state";

    // 6 messages: 3 user/assistant pairs (well-formed conversation)
    let messages: Vec<Message> = (0..3)
        .flat_map(|i| {
            [
                Message::user(format!("Entry {}", i * 2)),
                Message::assistant(format!("Reply {}", i * 2)),
            ]
        })
        .collect();

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });

    let config = CompactionParams {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingSummary,
        keep_recent: 2,
        token_budget: None,
    };

    let summarizer = |_: &[Message]| async move { Ok(("Compacted summary".to_string(), None)) };

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { super::compact(session_id, &config, &memory, summarizer).await })
        .unwrap();

    // Memory has LLM context (summary + 2 recent = 3 messages)
    let from_memory = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { memory.load(session_id).await.unwrap() });
    assert_eq!(from_memory.len(), 3); // summary + 2 recent

    // Store has full history: 6 original messages + 1 marker + 2 kept = 9 entries
    let (stored_entries, _) = memory.load_all(session_id).unwrap();
    assert_eq!(stored_entries.len(), 9);

    // extract_llm_context from store should produce the same messages as memory
    let context_from_store = extract_llm_context(&stored_entries);
    assert_eq!(from_memory.len(), context_from_store.len());
    for (mem_msg, store_msg) in from_memory.iter().zip(context_from_store.iter()) {
        let mem_json = serde_json::to_string(mem_msg).unwrap();
        let store_json = serde_json::to_string(store_msg).unwrap();
        assert_eq!(
            mem_json, store_json,
            "Memory and store-derived LLM context must match"
        );
    }
}

#[test]
fn compact_sliding_window_keeps_last_n_messages() {
    let temp_dir = TempDir::new().unwrap();
    let memory = JournalConversationMemory::new(temp_dir.path().to_path_buf());
    let session_id = "test_sliding_window";

    // 10 alternating messages (ends with assistant — survives repair unchanged)
    let messages = make_test_messages(10);

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });

    let config = CompactionParams {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingWindow,
        keep_recent: 3,
        token_budget: None,
    };

    // Summarizer should never be called — use a dummy
    let summarizer = |_: &[Message]| async move { Ok((String::new(), None)) };

    let outcome = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { super::compact(session_id, &config, &memory, summarizer).await })
        .unwrap();

    assert_eq!(outcome.summarized_count, 7);
    assert_eq!(outcome.kept_recent_count, 3);
    assert!(outcome.summary_text.is_empty());

    // Memory should have exactly 3 messages (no summary prepended)
    let final_messages = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { memory.load(session_id).await.unwrap() });

    assert_eq!(final_messages.len(), 3);

    // Store should have all original messages + 1 marker + 3 kept
    let (stored, _) = memory.load_all(session_id).unwrap();
    assert_eq!(stored.len(), 14); // 10 original + 1 marker + 3 kept

    // Marker should be at index 10
    match &stored[10] {
        StoreEntry::Marker(marker) => {
            assert!(marker.summary.is_empty());
            assert_eq!(marker.strategy, "sliding_window");
        }
        _ => panic!("Expected compaction marker at index 10"),
    }

    // All memory messages should be from the original conversation (user or assistant)
    for msg in &final_messages {
        assert!(
            matches!(msg, Message::User { .. } | Message::Assistant { .. }),
            "Expected user or assistant message (no system), got {:?}",
            msg
        );
    }
}

#[test]
fn compact_sliding_window_summarizer_not_called() {
    let temp_dir = TempDir::new().unwrap();
    let memory = JournalConversationMemory::new(temp_dir.path().to_path_buf());
    let session_id = "test_sliding_window_no_summarizer";

    // 10 alternating messages (ends with assistant — survives repair unchanged)
    let messages = make_test_messages(10);

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });

    let config = CompactionParams {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingWindow,
        keep_recent: 3,
        token_budget: None,
    };

    // Summarizer panics — if SlidingWindow ever calls it, test fails
    let summarizer = |_: &[Message]| async move {
        panic!("SlidingWindow must not call summarizer");
    };

    // Must complete without panic
    let outcome = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { super::compact(session_id, &config, &memory, summarizer).await })
        .unwrap();

    assert_eq!(outcome.kept_recent_count, 3);
    assert!(outcome.summary_text.is_empty());
}

#[test]
fn compact_sliding_window_preserves_message_order() {
    let temp_dir = TempDir::new().unwrap();
    let memory = JournalConversationMemory::new(temp_dir.path().to_path_buf());
    let session_id = "test_sliding_window_order";

    // 10 alternating messages (ends with assistant — survives repair unchanged)
    // make_test_messages(10) produces: msg0(u), msg1(a), ..., msg9(a)
    let messages = make_test_messages(10);

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });

    let config = CompactionParams {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingWindow,
        keep_recent: 4,
        token_budget: None,
    };

    let summarizer = |_: &[Message]| async move { Ok((String::new(), None)) };

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { super::compact(session_id, &config, &memory, summarizer).await })
        .unwrap();

    let final_messages = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { memory.load(session_id).await.unwrap() });

    assert_eq!(final_messages.len(), 4);

    // Verify messages are "msg6" through "msg9" in order
    let expected = ["msg6", "msg7", "msg8", "msg9"];
    for (msg, expected_text) in final_messages.iter().zip(expected.iter()) {
        let serialized = serde_json::to_string(msg).unwrap();
        assert!(
            serialized.contains(expected_text),
            "Expected message containing '{}', got: {}",
            expected_text,
            serialized
        );
    }
}

#[test]
fn compact_token_truncate_drops_oldest_within_budget() {
    let temp_dir = TempDir::new().unwrap();
    let memory = JournalConversationMemory::new(temp_dir.path().to_path_buf());
    let session_id = "test_token_truncate";

    // 6 alternating messages each with 400 chars of content = ~100 tokens each
    // (make_test_messages(5) rounds to 6; ends with assistant — survives repair)
    let messages: Vec<Message> = (0..6)
        .map(|i| {
            if i % 2 == 0 {
                Message::user(format!("{}{}", i, "x".repeat(400)))
            } else {
                Message::assistant(format!("{}{}", i, "x".repeat(400)))
            }
        })
        .collect();

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });

    let config = CompactionParams {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::TokenTruncate,
        keep_recent: 999, // ignored by TokenTruncate
        token_budget: Some(250),
    };

    // Panicking summarizer — TokenTruncate must never call it
    let summarizer = |_: &[Message]| async move {
        panic!("TokenTruncate must not call summarizer");
    };

    let _outcome = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { super::compact(session_id, &config, &memory, summarizer).await })
        .unwrap();

    // Verify: only newest messages kept within budget (~2 messages at ~100 tokens each)
    let final_messages = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { memory.load(session_id).await.unwrap() });

    assert_eq!(
        final_messages.len(),
        2,
        "Expected 2 messages within budget of 250 tokens"
    );
}

#[test]
fn compact_token_truncate_single_large_message() {
    let temp_dir = TempDir::new().unwrap();
    let memory = JournalConversationMemory::new(temp_dir.path().to_path_buf());
    let session_id = "test_token_truncate_single";

    // 1 large assistant message: 4000 chars = ~1000 tokens, well above budget.
    // An assistant message avoids trim_trailing_user in repair.
    let messages = vec![Message::assistant("x".repeat(4000))];

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });

    let config = CompactionParams {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::TokenTruncate,
        keep_recent: 999,
        token_budget: Some(100),
    };

    let summarizer = |_: &[Message]| async move {
        panic!("TokenTruncate must not call summarizer");
    };

    let _outcome = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { super::compact(session_id, &config, &memory, summarizer).await })
        .unwrap();

    // Must keep the message — never return empty, even if over budget
    let final_messages = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { memory.load(session_id).await.unwrap() });

    assert_eq!(
        final_messages.len(),
        1,
        "Must keep at least one message even if over budget"
    );
}

#[test]
fn compact_appends_marker_preserving_history() {
    // After compact, store.load_all() has all original messages + marker at end
    let temp_dir = TempDir::new().unwrap();
    let memory = JournalConversationMemory::new(temp_dir.path().to_path_buf());
    let session_id = "test_append_marker";

    // 6 alternating messages (make_test_messages(5) rounds to 6)
    let messages = make_test_messages(6);

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });

    let config = CompactionParams {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingSummary,
        keep_recent: 2,
        token_budget: None,
    };

    let summarizer = |_: &[Message]| async move { Ok(("Summary".to_string(), None)) };

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { super::compact(session_id, &config, &memory, summarizer).await })
        .unwrap();

    let (entries, _) = memory.load_all(session_id).unwrap();
    assert_eq!(entries.len(), 9); // 6 original + 1 marker + 2 kept

    // First 6 are messages
    for entry in &entries[..6] {
        assert!(
            matches!(entry, StoreEntry::Message(_)),
            "Expected message entry"
        );
    }

    // Index 6 is marker
    assert!(
        matches!(&entries[6], StoreEntry::Marker(_)),
        "Expected marker at index 6"
    );

    // Last 2 are re-appended kept messages
    for entry in &entries[7..9] {
        assert!(
            matches!(entry, StoreEntry::Message(_)),
            "Expected re-appended kept message"
        );
    }
}

#[test]
fn compact_marker_has_correct_fields() {
    // Check marker.summary, kept_recent_count, summarized_count, strategy
    let temp_dir = TempDir::new().unwrap();
    let memory = JournalConversationMemory::new(temp_dir.path().to_path_buf());
    let session_id = "test_marker_fields";

    // 8 alternating messages (ends with assistant — survives repair unchanged)
    let messages = make_test_messages(8);

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });

    let config = CompactionParams {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingSummary,
        keep_recent: 3,
        token_budget: None,
    };

    let summarizer = |old: &[Message]| {
        let count = old.len();
        async move { Ok((format!("Summarized {} old messages", count), None)) }
    };

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { super::compact(session_id, &config, &memory, summarizer).await })
        .unwrap();

    let (entries, _) = memory.load_all(session_id).unwrap();
    // Marker should be at index 8 (8 original msgs + marker at index 8, then 3 kept after)
    let marker = match &entries[8] {
        StoreEntry::Marker(m) => m,
        _ => panic!("Expected marker at index 8"),
    };

    assert_eq!(marker.summary, "Summarized 5 old messages");
    assert_eq!(marker.kept_recent_count, 3);
    assert_eq!(marker.summarized_count, 5);
    assert_eq!(marker.strategy, "sliding_summary");
}

#[test]
fn compact_memory_has_llm_context_only() {
    // Memory has summary + recent only, not the full history
    let temp_dir = TempDir::new().unwrap();
    let memory = JournalConversationMemory::new(temp_dir.path().to_path_buf());
    let session_id = "test_memory_llm_context";

    // 8 alternating messages (make_test_messages(7) rounds to 8)
    let messages = make_test_messages(8);

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });

    let config = CompactionParams {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingSummary,
        keep_recent: 2,
        token_budget: None,
    };

    let summarizer = |_: &[Message]| async move { Ok(("LLM summary".to_string(), None)) };

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { super::compact(session_id, &config, &memory, summarizer).await })
        .unwrap();

    let from_memory = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { memory.load(session_id).await.unwrap() });

    assert_eq!(from_memory.len(), 3); // summary + 2 recent

    // First is system summary
    match &from_memory[0] {
        Message::System { content } => assert_eq!(content, "LLM summary"),
        _ => panic!("Expected system message"),
    }

    // Remaining are user or assistant messages (mixed in alternating conversation)
    for msg in from_memory.iter().skip(1) {
        assert!(
            matches!(msg, Message::User { .. } | Message::Assistant { .. }),
            "Expected user or assistant message, got {:?}",
            msg
        );
    }
}

#[test]
fn compact_sliding_window_appends_marker_empty_summary() {
    // SlidingWindow marker has empty summary
    let temp_dir = TempDir::new().unwrap();
    let memory = JournalConversationMemory::new(temp_dir.path().to_path_buf());
    let session_id = "test_sw_marker_empty";

    // 6 alternating messages (ends with assistant — survives repair unchanged)
    let messages = make_test_messages(6);

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });

    let config = CompactionParams {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingWindow,
        keep_recent: 2,
        token_budget: None,
    };

    let summarizer = |_: &[Message]| async move { Ok((String::new(), None)) };

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { super::compact(session_id, &config, &memory, summarizer).await })
        .unwrap();

    let (entries, _) = memory.load_all(session_id).unwrap();
    // Marker at index 6 (6 original + marker), then 2 kept after
    let marker = match &entries[6] {
        StoreEntry::Marker(m) => m,
        _ => panic!("Expected marker at index 6"),
    };

    assert!(marker.summary.is_empty());
    assert_eq!(marker.strategy, "sliding_window");
}

#[test]
fn compact_token_truncate_appends_marker_empty_summary() {
    // TokenTruncate marker has empty summary
    let temp_dir = TempDir::new().unwrap();
    let memory = JournalConversationMemory::new(temp_dir.path().to_path_buf());
    let session_id = "test_tt_marker_empty";

    // 5 messages each with 400 chars = ~100 tokens each
    let messages: Vec<Message> = (0..5)
        .map(|i| Message::user(format!("{}{}", i, "x".repeat(400))))
        .collect();

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });

    let config = CompactionParams {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::TokenTruncate,
        keep_recent: 999,
        token_budget: Some(250),
    };

    let summarizer = |_: &[Message]| async move {
        panic!("TokenTruncate must not call summarizer");
    };

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { super::compact(session_id, &config, &memory, summarizer).await })
        .unwrap();

    let (entries, _) = memory.load_all(session_id).unwrap();
    // Marker at index 5 (5 original + marker), then kept messages after
    let marker = match &entries[5] {
        StoreEntry::Marker(m) => m,
        _ => panic!("Expected marker at index 5"),
    };

    assert!(marker.summary.is_empty());
    assert_eq!(marker.strategy, "token_truncate");
}

#[test]
fn multiple_compactions_append_multiple_markers() {
    // Compact twice, load_all shows original messages + 2 markers
    let temp_dir = TempDir::new().unwrap();
    let memory = JournalConversationMemory::new(temp_dir.path().to_path_buf());
    let session_id = "test_multi_marker";

    // 10 alternating messages (ends with assistant — survives repair unchanged)
    let messages = make_test_messages(10);

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });

    let config = CompactionParams {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingSummary,
        keep_recent: 3,
        token_budget: None,
    };

    // First compaction: 10 messages → 7 summarized, 3 kept
    let summarizer1 = |_: &[Message]| async move { Ok(("Summary 1".to_string(), None)) };
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { super::compact(session_id, &config, &memory, summarizer1).await })
        .unwrap();

    // After first compact, memory has 4 messages (summary + 3 recent).
    // Add 4 more alternating messages to trigger second compaction (4 + 4 = 8 > keep_recent=3).
    let more_messages = make_test_messages(4); // ends with assistant

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory
            .append(session_id, more_messages.clone())
            .await
            .unwrap();
    });

    // Second compaction
    let summarizer2 = |_: &[Message]| async move { Ok(("Summary 2".to_string(), None)) };
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { super::compact(session_id, &config, &memory, summarizer2).await })
        .unwrap();

    let (entries, _) = memory.load_all(session_id).unwrap();

    // Count markers
    let marker_count = entries
        .iter()
        .filter(|e| matches!(e, StoreEntry::Marker(_)))
        .count();
    assert_eq!(marker_count, 2, "Expected 2 compaction markers");
}

#[test]
fn compact_writes_null_tokens_to_store() {
    // After compact, load_all returns None for last_total_tokens because
    // the marker and kept messages are written with None. The stale pre-compaction
    // value is intentionally discarded; the real post-compaction value comes from
    // the summarizer's streaming Final usage.
    let temp_dir = TempDir::new().unwrap();
    let memory = JournalConversationMemory::new(temp_dir.path().to_path_buf());
    let session_id = "test_token_preservation";

    // 6 alternating messages (ends with assistant — survives repair unchanged)
    let messages = make_test_messages(6);

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });

    let config = CompactionParams {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingWindow,
        keep_recent: 2,
        token_budget: None,
    };

    // Compact — marker and kept messages should have null tokens
    let summarizer = |_: &[Message]| async move { Ok((String::new(), None)) };
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { super::compact(session_id, &config, &memory, summarizer).await })
        .unwrap();

    // Verify load_all returns None — pre-compaction tokens are not written to store
    let (_, last_tokens) = memory.load_all(session_id).unwrap();
    assert_eq!(last_tokens, None);
}

#[tokio::test]
async fn compact_no_double_write_kept_messages() {
    let temp_dir = TempDir::new().unwrap();
    let memory = JournalConversationMemory::new(temp_dir.path().to_path_buf());
    let session_id = "test_no_double_write";
    let config = CompactionParams {
        compaction_threshold: 5,
        keep_recent: 3,
        compaction_strategy: CompactionStrategy::SlidingSummary,
        token_budget: None,
    };
    // 10 alternating messages (ends with assistant — survives repair unchanged)
    let messages = make_test_messages(10);
    memory.append(session_id, messages).await.unwrap();
    let summarizer =
        |_: &[Message]| async { Ok::<_, std::io::Error>(("summary".to_string(), None)) };
    super::compact(session_id, &config, &memory, summarizer)
        .await
        .unwrap();
    let (entries, _) = memory.load_all(session_id).unwrap();
    let marker_idx = entries
        .iter()
        .rposition(|e| matches!(e, StoreEntry::Marker(_)))
        .unwrap();
    let after_marker = entries[marker_idx + 1..]
        .iter()
        .filter(|e| matches!(e, StoreEntry::Message(_)))
        .count();
    assert_eq!(
        after_marker, 3,
        "kept messages must appear exactly once after marker, not doubled"
    );
}

#[tokio::test]
async fn compact_clear_does_not_delete_jsonl() {
    let temp_dir = TempDir::new().unwrap();
    let memory = JournalConversationMemory::new(temp_dir.path().to_path_buf());
    let session_id = "test_clear_no_delete";
    let config = CompactionParams {
        compaction_threshold: 3,
        keep_recent: 2,
        compaction_strategy: CompactionStrategy::SlidingSummary,
        token_budget: None,
    };
    // 6 alternating messages (make_test_messages(5) rounds to 6)
    let messages = make_test_messages(6);
    memory.append(session_id, messages).await.unwrap();
    let summarizer =
        |_: &[Message]| async { Ok::<_, std::io::Error>(("summary".to_string(), None)) };
    super::compact(session_id, &config, &memory, summarizer)
        .await
        .unwrap();
    let (entries, _) = memory.load_all(session_id).unwrap();
    assert!(!entries.is_empty(), "JSONL must not be empty after compact");
    assert!(
        entries.iter().any(|e| matches!(e, StoreEntry::Marker(_))),
        "JSONL must contain a compaction marker"
    );
}

#[tokio::test]
async fn compact_rollback_clears_cache() {
    let temp_dir = TempDir::new().unwrap();
    let memory = JournalConversationMemory::new(temp_dir.path().to_path_buf());
    let session_id = "test_rollback";
    let config = CompactionParams {
        compaction_threshold: 3,
        keep_recent: 2,
        compaction_strategy: CompactionStrategy::SlidingSummary,
        token_budget: None,
    };
    // 6 messages: 3 user/assistant pairs (well-formed conversation)
    let messages: Vec<Message> = (0..3)
        .flat_map(|i| {
            [
                Message::user(format!("msg {i}")),
                Message::assistant(format!("reply {i}")),
            ]
        })
        .collect();
    memory.append(session_id, messages).await.unwrap();
    let summarizer =
        |_: &[Message]| async { Ok::<_, std::io::Error>(("summary".to_string(), None)) };
    super::compact(session_id, &config, &memory, summarizer)
        .await
        .unwrap();
    let from_cache = memory.load(session_id).await.unwrap();
    assert!(!from_cache.is_empty());
    memory.clear(session_id).await.unwrap();
    let from_jsonl = memory.load(session_id).await.unwrap();
    assert_eq!(
        from_cache.len(),
        from_jsonl.len(),
        "re-loaded context must match compacted context"
    );
}

#[test]
fn compact_writes_null_tokens_to_marker_and_kept_messages() {
    // After compact, the JSONL file must have null last_total_tokens on the marker
    // and post-marker (kept) messages. The stale pre-compaction value must not be
    // persisted — the real post-compaction count comes from the streaming Final chunk.
    let temp_dir = TempDir::new().unwrap();
    let memory = JournalConversationMemory::new(temp_dir.path().to_path_buf());
    let session_id = "test_null_tokens_marker";

    // 6 alternating messages (ends with assistant — survives repair unchanged)
    let messages = make_test_messages(6);

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });

    let config = CompactionParams {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingSummary,
        keep_recent: 2,
        token_budget: None,
    };

    let summarizer = |_: &[Message]| async move { Ok(("Summary".to_string(), Some(5000u64))) };

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { super::compact(session_id, &config, &memory, summarizer).await })
        .unwrap();

    // load_all returns the last non-null last_total_tokens from the file.
    // Since marker and kept messages are written with None, no token value exists
    // after the marker in the file.
    let (entries, last_tokens) = memory.load_all(session_id).unwrap();

    // Marker and kept messages written with None → load_all returns None
    assert_eq!(
        last_tokens, None,
        "marker and kept messages must have null tokens in JSONL"
    );

    // Verify entries contain a marker and kept messages (sanity check)
    let marker_idx = entries
        .iter()
        .rposition(|e| matches!(e, StoreEntry::Marker(_)))
        .expect("expected a marker");
    let after_marker: Vec<_> = entries[marker_idx + 1..]
        .iter()
        .filter(|e| matches!(e, StoreEntry::Message(_)))
        .collect();
    assert_eq!(
        after_marker.len(),
        2,
        "expected 2 kept messages after marker"
    );

    // Inspect the raw JSONL file to confirm null fields
    let file_path = temp_dir.path().join(format!("{}.jsonl", session_id));
    let contents = std::fs::read_to_string(file_path).unwrap();
    let lines: Vec<&str> = contents.lines().collect();

    // Lines after the first (metadata) should include a marker and kept messages.
    // None of the lines after the original 6 messages should contain last_total_tokens.
    for line in lines.iter().skip(7) {
        // skip metadata + 6 original messages
        let value: serde_json::Value = serde_json::from_str(line).unwrap();
        assert!(
            value.get("last_total_tokens").is_none(),
            "marker/kept lines must not have last_total_tokens field: {line}"
        );
    }
}

#[test]
fn compact_outcome_includes_summary_tokens() {
    // compact() outcome.summary_total_tokens must match what the summarizer returns
    let temp_dir = TempDir::new().unwrap();
    let memory = JournalConversationMemory::new(temp_dir.path().to_path_buf());
    let session_id = "test_outcome_tokens";

    let messages = make_test_messages(6);

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages).await.unwrap();
    });

    let config = CompactionParams {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingSummary,
        keep_recent: 2,
        token_budget: None,
    };

    // Summarizer returns token count of 5000
    let summarizer = |_: &[Message]| async move { Ok(("summary".to_string(), Some(5000u64))) };

    let outcome = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { super::compact(session_id, &config, &memory, summarizer).await })
        .unwrap();

    assert_eq!(
        outcome.summary_total_tokens,
        Some(5000),
        "CompactionOutcome must carry the token count from the summarizer"
    );
}
