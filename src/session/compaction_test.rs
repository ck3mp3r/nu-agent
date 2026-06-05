use super::*;
use rig::completion::Message;
use rig::completion::message::{
    AssistantContent, Text, ToolCall, ToolFunction, ToolResultContent, UserContent,
};
use rig::memory::{ConversationMemory, InMemoryConversationMemory};
use rig::one_or_many::OneOrMany;
use serde_json::json;
use std::sync::Arc;
use tempfile::TempDir;

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
        content: OneOrMany::one(UserContent::ToolResult(
            rig::completion::message::ToolResult {
                id: call_id.to_string(),
                call_id: None,
                content: OneOrMany::one(ToolResultContent::Text(Text {
                    text: result_text.to_string(),
                })),
            },
        )),
    }
}

#[test]
fn compact_splits_at_keep_recent() {
    // RED: Test that compaction loads from memory, splits correctly, and stores back
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    let memory = Arc::new(InMemoryConversationMemory::new());
    let session_id = "test_session";

    // Setup: 10 messages in memory
    let messages: Vec<Message> = (0..10)
        .map(|i| Message::user(format!("Message {}", i)))
        .collect();

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });

    let keep_recent = 3;
    let config = SessionConfig {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingSummary,
        keep_recent,
        token_budget: None,
    };

    let mut session = Session {
        id: session_id.to_string(),
        created_at: Utc::now(),
        config,
        compaction_count: 0,
    };

    // Mock summarizer that formats old messages
    let summarizer = |old_messages: &[Message]| {
        let count = old_messages.len();
        async move { Ok(format!("Summary of {} messages", count)) }
    };

    let outcome = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { session.compact(&memory, &store, summarizer).await })
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

    // Last 3 should be the recent user messages (7, 8, 9)
    // Just verify they are user messages - content inspection is complex with OneOrMany
    for msg in final_messages.iter().skip(1) {
        match msg {
            Message::User { .. } => {
                // Content exists and is a user message - good enough
            }
            _ => panic!("Expected user message"),
        }
    }
}

#[test]
fn compact_persists_to_store() {
    // RED: Test that compacted messages are persisted to ConversationStore
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    let memory = Arc::new(InMemoryConversationMemory::new());
    let session_id = "test_persist";

    // Setup: 5 messages
    let messages: Vec<Message> = (0..5)
        .map(|i| Message::user(format!("Msg {}", i)))
        .collect();

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });
    store.append(session_id, &messages).unwrap();

    let config = SessionConfig {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingSummary,
        keep_recent: 2,
        token_budget: None,
    };

    let mut session = Session {
        id: session_id.to_string(),
        created_at: Utc::now(),
        config,
        compaction_count: 0,
    };

    let summarizer = |_: &[Message]| async move { Ok("Summary".to_string()) };

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { session.compact(&memory, &store, summarizer).await })
        .unwrap();

    // Verify: store contains all original messages + compaction marker
    let stored = store.load_all(session_id).unwrap();
    assert_eq!(stored.len(), 6); // 5 original + 1 marker

    // Last entry should be a compaction marker
    match &stored[5] {
        StoreEntry::Marker(marker) => {
            assert_eq!(marker.summary, "Summary");
            assert_eq!(marker.kept_recent_count, 2);
        }
        _ => panic!("Expected compaction marker as last entry"),
    }
}

#[test]
fn compact_increments_compaction_count() {
    // RED: Test that compaction_count is incremented
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    let memory = Arc::new(InMemoryConversationMemory::new());
    let session_id = "test_count";

    let messages: Vec<Message> = (0..5).map(|i| Message::user(format!("M{}", i))).collect();

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });

    let config = SessionConfig {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingSummary,
        keep_recent: 2,
        token_budget: None,
    };

    let mut session = Session {
        id: session_id.to_string(),
        created_at: Utc::now(),
        config,
        compaction_count: 0,
    };

    assert_eq!(session.compaction_count, 0);

    let summarizer = |_: &[Message]| async move { Ok("S".to_string()) };

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { session.compact(&memory, &store, summarizer).await })
        .unwrap();

    assert_eq!(session.compaction_count, 1);
}

#[test]
fn compact_handles_insufficient_messages() {
    // RED: Test no-op when messages <= keep_recent
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    let memory = Arc::new(InMemoryConversationMemory::new());
    let session_id = "test_noop";

    // Setup: only 2 messages, keep_recent = 3
    let messages = vec![Message::user("A"), Message::user("B")];

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });

    let config = SessionConfig {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingSummary,
        keep_recent: 3,
        token_budget: None,
    };

    let mut session = Session {
        id: session_id.to_string(),
        created_at: Utc::now(),
        config,
        compaction_count: 0,
    };

    let summarizer = |_: &[Message]| async move { Ok("Summary".to_string()) };

    let outcome = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { session.compact(&memory, &store, summarizer).await })
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
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    let memory = Arc::new(InMemoryConversationMemory::new());
    let session_id = "test_clear";

    let messages: Vec<Message> = (0..5).map(|i| Message::user(format!("X{}", i))).collect();

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });

    let config = SessionConfig {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingSummary,
        keep_recent: 2,
        token_budget: None,
    };

    let mut session = Session {
        id: session_id.to_string(),
        created_at: Utc::now(),
        config,
        compaction_count: 0,
    };

    let summarizer = |old: &[Message]| {
        let count = old.len();
        async move { Ok(format!("Summarized {}", count)) }
    };

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { session.compact(&memory, &store, summarizer).await })
        .unwrap();

    // Memory should contain exactly 3 messages (summary + 2 recent), not 5 + 3
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
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    let memory = Arc::new(InMemoryConversationMemory::new());
    let session_id = "test_async_no_panic";

    let messages: Vec<Message> = (0..5)
        .map(|i| Message::user(format!("Msg {}", i)))
        .collect();

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        memory.append(session_id, messages).await.unwrap();
    });

    let config = SessionConfig {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingSummary,
        keep_recent: 2,
        token_budget: None,
    };

    let mut session = Session {
        id: session_id.to_string(),
        created_at: Utc::now(),
        config,
        compaction_count: 0,
    };

    // Async summarizer that actually awaits something — this is the key.
    // A sync closure wrapped in async wouldn't trigger the bug.
    let summarizer = |old: &[Message]| {
        let count = old.len();
        async move {
            // Yield to the runtime — this ensures we're truly async
            tokio::task::yield_now().await;
            Ok(format!("Summarized {} messages", count))
        }
    };

    // This is the production pattern: block_on wrapping an async call
    // that internally awaits the summarizer
    let outcome = rt
        .block_on(async { session.compact(&memory, &store, summarizer).await })
        .unwrap();

    assert_eq!(outcome.summarized_count, 3);
    assert_eq!(outcome.kept_recent_count, 2);
}

// --- find_safe_split_index unit tests ---

#[test]
fn find_safe_split_index_adjusts_for_tool_result_at_boundary() {
    // [user, user, TC_A, TR_A, user, user]
    // target=3 (TR_A is at index 3) → should move back to 2
    let messages = vec![
        Message::user("u0"),
        Message::user("u1"),
        make_tool_call_message("A", "read"),
        make_tool_result_message("A", "ok"),
        Message::user("u2"),
        Message::user("u3"),
    ];
    assert_eq!(find_safe_split_index(&messages, 3), 2);
}

#[test]
fn find_safe_split_index_adjusts_for_consecutive_pairs() {
    // [user, TC_A, TR_A, TC_B, TR_B, user]
    // target=4 (TR_B at index 4) → walks back to 3 (between TR_A and TC_B is safe)
    let messages = vec![
        Message::user("u0"),
        make_tool_call_message("A", "read"),
        make_tool_result_message("A", "ok"),
        make_tool_call_message("B", "write"),
        make_tool_result_message("B", "ok"),
        Message::user("u1"),
    ];
    assert_eq!(find_safe_split_index(&messages, 4), 3);
}

#[test]
fn find_safe_split_index_no_adjustment_when_clean() {
    // [user, user, user, user]
    // target=2 → no tool pairs, stays at 2
    let messages = vec![
        Message::user("u0"),
        Message::user("u1"),
        Message::user("u2"),
        Message::user("u3"),
    ];
    assert_eq!(find_safe_split_index(&messages, 2), 2);
}

#[test]
fn find_safe_split_index_all_tool_pairs() {
    // [TC_A, TR_A, TC_B, TR_B]
    // target=2 → boundary between TR_A and TC_B is safe, stays at 2
    let messages = vec![
        make_tool_call_message("A", "read"),
        make_tool_result_message("A", "ok"),
        make_tool_call_message("B", "write"),
        make_tool_result_message("B", "ok"),
    ];
    assert_eq!(find_safe_split_index(&messages, 2), 2);
}

#[test]
fn compact_does_not_split_tool_call_result_pair() {
    // Integration test: 10 messages with tool pair at indices 6-7, keep_recent=3.
    // Naive split_index = 10 - 3 = 7, which would cut between TC and TR.
    // Safe split should move back to index 6, keeping both in the recent window.
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    let memory = Arc::new(InMemoryConversationMemory::new());
    let session_id = "test_tool_pair_split";

    let messages: Vec<Message> = vec![
        Message::user("m0"),
        Message::user("m1"),
        Message::user("m2"),
        Message::user("m3"),
        Message::user("m4"),
        Message::user("m5"),
        make_tool_call_message("tc1", "read_file"),
        make_tool_result_message("tc1", "file contents"),
        Message::user("m8"),
        Message::user("m9"),
    ];

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });

    let config = SessionConfig {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingSummary,
        keep_recent: 3,
        token_budget: None,
    };

    let mut session = Session {
        id: session_id.to_string(),
        created_at: Utc::now(),
        config,
        compaction_count: 0,
    };

    let summarizer = |old: &[Message]| {
        let count = old.len();
        async move { Ok(format!("Summary of {} messages", count)) }
    };

    let outcome = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { session.compact(&memory, &store, summarizer).await })
        .unwrap();

    // Naive split at 7 → safe split at 6 → 6 summarized, 4 kept
    assert_eq!(outcome.summarized_count, 6);
    assert_eq!(outcome.kept_recent_count, 4);

    // Verify memory: summary + 4 recent = 5 messages
    let final_messages = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { memory.load(session_id).await.unwrap() });

    assert_eq!(final_messages.len(), 5);

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
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    let memory = Arc::new(InMemoryConversationMemory::new());
    let session_id = "test_store_first";

    let messages: Vec<Message> = (0..8)
        .map(|i| Message::user(format!("Msg {}", i)))
        .collect();

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });
    store.append(session_id, &messages).unwrap();

    let config = SessionConfig {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingSummary,
        keep_recent: 3,
        token_budget: None,
    };

    let mut session = Session {
        id: session_id.to_string(),
        created_at: Utc::now(),
        config,
        compaction_count: 0,
    };

    let summarizer = |old: &[Message]| {
        let count = old.len();
        async move { Ok(format!("Summary of {} messages", count)) }
    };

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { session.compact(&memory, &store, summarizer).await })
        .unwrap();

    // Store must have all original messages + compaction marker
    let stored = store.load_all(session_id).unwrap();
    assert_eq!(stored.len(), 9); // 8 original + 1 marker

    // Last entry should be a compaction marker
    match &stored[8] {
        StoreEntry::Marker(marker) => {
            assert_eq!(marker.summary, "Summary of 5 messages");
            assert_eq!(marker.kept_recent_count, 3);
        }
        _ => panic!("Expected compaction marker as last entry"),
    }
}

#[test]
fn compact_successful_produces_correct_state() {
    // After a successful compact, memory has LLM context (summary + recent)
    // and store has full append-only history (all messages + marker).
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    let memory = Arc::new(InMemoryConversationMemory::new());
    let session_id = "test_consistent_state";

    let messages: Vec<Message> = (0..6)
        .map(|i| Message::user(format!("Entry {}", i)))
        .collect();

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });
    store.append(session_id, &messages).unwrap();

    let config = SessionConfig {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingSummary,
        keep_recent: 2,
        token_budget: None,
    };

    let mut session = Session {
        id: session_id.to_string(),
        created_at: Utc::now(),
        config,
        compaction_count: 0,
    };

    let summarizer = |_: &[Message]| async move { Ok("Compacted summary".to_string()) };

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { session.compact(&memory, &store, summarizer).await })
        .unwrap();

    // Memory has LLM context (summary + 2 recent = 3 messages)
    let from_memory = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { memory.load(session_id).await.unwrap() });
    assert_eq!(from_memory.len(), 3); // summary + 2 recent

    // Store has full history: 6 original messages + 1 marker = 7 entries
    let stored_entries = store.load_all(session_id).unwrap();
    assert_eq!(stored_entries.len(), 7);

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

// --- SlidingWindow compaction tests ---

#[test]
fn compact_sliding_window_keeps_last_n_messages() {
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    let memory = Arc::new(InMemoryConversationMemory::new());
    let session_id = "test_sliding_window";

    let messages: Vec<Message> = (0..10)
        .map(|i| Message::user(format!("msg{}", i)))
        .collect();

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });
    store.append(session_id, &messages).unwrap();

    let config = SessionConfig {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingWindow,
        keep_recent: 3,
        token_budget: None,
    };

    let mut session = Session {
        id: session_id.to_string(),
        created_at: Utc::now(),
        config,
        compaction_count: 0,
    };

    // Summarizer should never be called — use a dummy
    let summarizer = |_: &[Message]| async move { Ok(String::new()) };

    let outcome = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { session.compact(&memory, &store, summarizer).await })
        .unwrap();

    assert_eq!(outcome.summarized_count, 7);
    assert_eq!(outcome.kept_recent_count, 3);
    assert!(outcome.summary_text.is_empty());

    // Memory should have exactly 3 messages (no summary prepended)
    let final_messages = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { memory.load(session_id).await.unwrap() });

    assert_eq!(final_messages.len(), 3);

    // Store should have all original messages + 1 marker
    let stored = store.load_all(session_id).unwrap();
    assert_eq!(stored.len(), 11); // 10 original + 1 marker

    // Last entry should be a compaction marker with empty summary
    match stored.last().unwrap() {
        StoreEntry::Marker(marker) => {
            assert!(marker.summary.is_empty());
            assert_eq!(marker.strategy, "sliding_window");
        }
        _ => panic!("Expected compaction marker as last entry"),
    }

    // All memory messages should be user messages (no system summary)
    for msg in &final_messages {
        assert!(
            matches!(msg, Message::User { .. }),
            "Expected user message, got {:?}",
            msg
        );
    }
}

#[test]
fn compact_sliding_window_summarizer_not_called() {
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    let memory = Arc::new(InMemoryConversationMemory::new());
    let session_id = "test_sliding_window_no_summarizer";

    let messages: Vec<Message> = (0..10)
        .map(|i| Message::user(format!("msg{}", i)))
        .collect();

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });

    let config = SessionConfig {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingWindow,
        keep_recent: 3,
        token_budget: None,
    };

    let mut session = Session {
        id: session_id.to_string(),
        created_at: Utc::now(),
        config,
        compaction_count: 0,
    };

    // Summarizer panics — if SlidingWindow ever calls it, test fails
    let summarizer = |_: &[Message]| async move {
        panic!("SlidingWindow must not call summarizer");
    };

    // Must complete without panic
    let outcome = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { session.compact(&memory, &store, summarizer).await })
        .unwrap();

    assert_eq!(outcome.kept_recent_count, 3);
    assert!(outcome.summary_text.is_empty());
}

#[test]
fn compact_sliding_window_preserves_message_order() {
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    let memory = Arc::new(InMemoryConversationMemory::new());
    let session_id = "test_sliding_window_order";

    let messages: Vec<Message> = (0..10)
        .map(|i| Message::user(format!("msg{}", i)))
        .collect();

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });

    let config = SessionConfig {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingWindow,
        keep_recent: 4,
        token_budget: None,
    };

    let mut session = Session {
        id: session_id.to_string(),
        created_at: Utc::now(),
        config,
        compaction_count: 0,
    };

    let summarizer = |_: &[Message]| async move { Ok(String::new()) };

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { session.compact(&memory, &store, summarizer).await })
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

// --- TokenTruncate compaction tests ---

#[test]
fn estimate_tokens_approximation() {
    // "hello world" = 11 chars → serialized in a Message::user has overhead,
    // but raw "hello world" text = 11 chars → 2 tokens (11/4 = 2 truncated).
    // We test via Message::user which adds JSON overhead, so we test the
    // function's proportionality instead.
    let msg_small = Message::user("hello world");
    let tokens_small = estimate_tokens(&msg_small);
    // "hello world" is 11 chars but serialized JSON is larger; verify > 0
    assert!(tokens_small > 0, "Expected non-zero tokens for 'hello world'");

    let msg_large = Message::user("x".repeat(400));
    let tokens_large = estimate_tokens(&msg_large);
    // 400 chars of content → ~100 tokens of content + JSON overhead
    assert!(tokens_large >= 100, "Expected >= 100 tokens for 400 chars, got {}", tokens_large);

    // Large message should have significantly more tokens
    assert!(tokens_large > tokens_small, "Larger message should have more tokens");
}

#[test]
fn compact_token_truncate_drops_oldest_within_budget() {
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    let memory = Arc::new(InMemoryConversationMemory::new());
    let session_id = "test_token_truncate";

    // 5 messages each with 400 chars = ~100 tokens of content each
    let messages: Vec<Message> = (0..5)
        .map(|i| Message::user(format!("{}{}", i, "x".repeat(400))))
        .collect();

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });

    let config = SessionConfig {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::TokenTruncate,
        keep_recent: 999, // ignored by TokenTruncate
        token_budget: Some(250),
    };

    let mut session = Session {
        id: session_id.to_string(),
        created_at: Utc::now(),
        config,
        compaction_count: 0,
    };

    // Panicking summarizer — TokenTruncate must never call it
    let summarizer = |_: &[Message]| async move {
        panic!("TokenTruncate must not call summarizer");
    };

    let _outcome = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { session.compact(&memory, &store, summarizer).await })
        .unwrap();

    // Verify: only newest messages kept within budget
    let final_messages = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { memory.load(session_id).await.unwrap() });

    assert_eq!(final_messages.len(), 2, "Expected 2 messages within budget of 250 tokens");
}

#[test]
fn compact_token_truncate_single_large_message() {
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    let memory = Arc::new(InMemoryConversationMemory::new());
    let session_id = "test_token_truncate_single";

    // 1 message with 4000 chars = ~1000 tokens, budget = 100
    let messages = vec![Message::user("x".repeat(4000))];

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });

    let config = SessionConfig {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::TokenTruncate,
        keep_recent: 999,
        token_budget: Some(100),
    };

    let mut session = Session {
        id: session_id.to_string(),
        created_at: Utc::now(),
        config,
        compaction_count: 0,
    };

    let summarizer = |_: &[Message]| async move {
        panic!("TokenTruncate must not call summarizer");
    };

    let _outcome = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { session.compact(&memory, &store, summarizer).await })
        .unwrap();

    // Must keep the message — never return empty
    let final_messages = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { memory.load(session_id).await.unwrap() });

    assert_eq!(final_messages.len(), 1, "Must keep at least one message even if over budget");
}

// --- Append-only marker architecture tests ---

#[test]
fn compact_appends_marker_preserving_history() {
    // After compact, store.load_all() has all original messages + marker at end
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    let memory = Arc::new(InMemoryConversationMemory::new());
    let session_id = "test_append_marker";

    let messages: Vec<Message> = (0..5)
        .map(|i| Message::user(format!("Msg {}", i)))
        .collect();

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });
    store.append(session_id, &messages).unwrap();

    let config = SessionConfig {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingSummary,
        keep_recent: 2,
        token_budget: None,
    };

    let mut session = Session {
        id: session_id.to_string(),
        created_at: Utc::now(),
        config,
        compaction_count: 0,
    };

    let summarizer = |_: &[Message]| async move { Ok("Summary".to_string()) };

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { session.compact(&memory, &store, summarizer).await })
        .unwrap();

    let entries = store.load_all(session_id).unwrap();
    assert_eq!(entries.len(), 6); // 5 original + 1 marker

    // First 5 are messages
    for entry in &entries[..5] {
        assert!(
            matches!(entry, StoreEntry::Message(_)),
            "Expected message entry"
        );
    }

    // Last is marker
    assert!(
        matches!(&entries[5], StoreEntry::Marker(_)),
        "Expected marker as last entry"
    );
}

#[test]
fn compact_marker_has_correct_fields() {
    // Check marker.summary, kept_recent_count, summarized_count, strategy
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    let memory = Arc::new(InMemoryConversationMemory::new());
    let session_id = "test_marker_fields";

    let messages: Vec<Message> = (0..8)
        .map(|i| Message::user(format!("Msg {}", i)))
        .collect();

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });
    store.append(session_id, &messages).unwrap();

    let config = SessionConfig {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingSummary,
        keep_recent: 3,
        token_budget: None,
    };

    let mut session = Session {
        id: session_id.to_string(),
        created_at: Utc::now(),
        config,
        compaction_count: 0,
    };

    let summarizer = |old: &[Message]| {
        let count = old.len();
        async move { Ok(format!("Summarized {} old messages", count)) }
    };

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { session.compact(&memory, &store, summarizer).await })
        .unwrap();

    let entries = store.load_all(session_id).unwrap();
    let marker = match entries.last().unwrap() {
        StoreEntry::Marker(m) => m,
        _ => panic!("Expected marker as last entry"),
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
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    let memory = Arc::new(InMemoryConversationMemory::new());
    let session_id = "test_memory_llm_context";

    let messages: Vec<Message> = (0..7)
        .map(|i| Message::user(format!("Msg {}", i)))
        .collect();

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });
    store.append(session_id, &messages).unwrap();

    let config = SessionConfig {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingSummary,
        keep_recent: 2,
        token_budget: None,
    };

    let mut session = Session {
        id: session_id.to_string(),
        created_at: Utc::now(),
        config,
        compaction_count: 0,
    };

    let summarizer = |_: &[Message]| async move { Ok("LLM summary".to_string()) };

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { session.compact(&memory, &store, summarizer).await })
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

    // Remaining are user messages
    for msg in from_memory.iter().skip(1) {
        assert!(
            matches!(msg, Message::User { .. }),
            "Expected user message"
        );
    }
}

#[test]
fn compact_sliding_window_appends_marker_empty_summary() {
    // SlidingWindow marker has empty summary
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    let memory = Arc::new(InMemoryConversationMemory::new());
    let session_id = "test_sw_marker_empty";

    let messages: Vec<Message> = (0..6)
        .map(|i| Message::user(format!("Msg {}", i)))
        .collect();

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });
    store.append(session_id, &messages).unwrap();

    let config = SessionConfig {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingWindow,
        keep_recent: 2,
        token_budget: None,
    };

    let mut session = Session {
        id: session_id.to_string(),
        created_at: Utc::now(),
        config,
        compaction_count: 0,
    };

    let summarizer = |_: &[Message]| async move { Ok(String::new()) };

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { session.compact(&memory, &store, summarizer).await })
        .unwrap();

    let entries = store.load_all(session_id).unwrap();
    let marker = match entries.last().unwrap() {
        StoreEntry::Marker(m) => m,
        _ => panic!("Expected marker"),
    };

    assert!(marker.summary.is_empty());
    assert_eq!(marker.strategy, "sliding_window");
}

#[test]
fn compact_token_truncate_appends_marker_empty_summary() {
    // TokenTruncate marker has empty summary
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    let memory = Arc::new(InMemoryConversationMemory::new());
    let session_id = "test_tt_marker_empty";

    // 5 messages each with 400 chars = ~100 tokens each
    let messages: Vec<Message> = (0..5)
        .map(|i| Message::user(format!("{}{}", i, "x".repeat(400))))
        .collect();

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });
    store.append(session_id, &messages).unwrap();

    let config = SessionConfig {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::TokenTruncate,
        keep_recent: 999,
        token_budget: Some(250),
    };

    let mut session = Session {
        id: session_id.to_string(),
        created_at: Utc::now(),
        config,
        compaction_count: 0,
    };

    let summarizer = |_: &[Message]| async move {
        panic!("TokenTruncate must not call summarizer");
    };

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { session.compact(&memory, &store, summarizer).await })
        .unwrap();

    let entries = store.load_all(session_id).unwrap();
    let marker = match entries.last().unwrap() {
        StoreEntry::Marker(m) => m,
        _ => panic!("Expected marker"),
    };

    assert!(marker.summary.is_empty());
    assert_eq!(marker.strategy, "token_truncate");
}

#[test]
fn multiple_compactions_append_multiple_markers() {
    // Compact twice, load_all shows original messages + 2 markers
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    let memory = Arc::new(InMemoryConversationMemory::new());
    let session_id = "test_multi_marker";

    let messages: Vec<Message> = (0..10)
        .map(|i| Message::user(format!("Msg {}", i)))
        .collect();

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });
    store.append(session_id, &messages).unwrap();

    let config = SessionConfig {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingSummary,
        keep_recent: 3,
        token_budget: None,
    };

    let mut session = Session {
        id: session_id.to_string(),
        created_at: Utc::now(),
        config,
        compaction_count: 0,
    };

    // First compaction: 10 messages → 7 summarized, 3 kept
    let summarizer1 = |_: &[Message]| async move { Ok("Summary 1".to_string()) };
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { session.compact(&memory, &store, summarizer1).await })
        .unwrap();

    // After first compact, memory has 4 messages (summary + 3 recent).
    // Add more messages to trigger second compaction.
    let more_messages: Vec<Message> = (10..16)
        .map(|i| Message::user(format!("Msg {}", i)))
        .collect();

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, more_messages.clone()).await.unwrap();
    });
    store.append(session_id, &more_messages).unwrap();

    // Second compaction
    let summarizer2 = |_: &[Message]| async move { Ok("Summary 2".to_string()) };
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { session.compact(&memory, &store, summarizer2).await })
        .unwrap();

    let entries = store.load_all(session_id).unwrap();

    // Count markers
    let marker_count = entries
        .iter()
        .filter(|e| matches!(e, StoreEntry::Marker(_)))
        .count();
    assert_eq!(marker_count, 2, "Expected 2 compaction markers");
}

#[test]
fn compact_increments_in_memory_count_marker() {
    // session.compaction_count goes 0 → 1 → 2
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    let memory = Arc::new(InMemoryConversationMemory::new());
    let session_id = "test_count_marker";

    let messages: Vec<Message> = (0..6)
        .map(|i| Message::user(format!("Msg {}", i)))
        .collect();

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, messages.clone()).await.unwrap();
    });
    store.append(session_id, &messages).unwrap();

    let config = SessionConfig {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingSummary,
        keep_recent: 2,
        token_budget: None,
    };

    let mut session = Session {
        id: session_id.to_string(),
        created_at: Utc::now(),
        config,
        compaction_count: 0,
    };

    assert_eq!(session.compaction_count, 0);

    // First compaction
    let summarizer1 = |_: &[Message]| async move { Ok("S1".to_string()) };
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { session.compact(&memory, &store, summarizer1).await })
        .unwrap();
    assert_eq!(session.compaction_count, 1);

    // Add more messages for second compaction
    let more: Vec<Message> = (6..12)
        .map(|i| Message::user(format!("Msg {}", i)))
        .collect();
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        memory.append(session_id, more.clone()).await.unwrap();
    });
    store.append(session_id, &more).unwrap();

    // Second compaction
    let summarizer2 = |_: &[Message]| async move { Ok("S2".to_string()) };
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { session.compact(&memory, &store, summarizer2).await })
        .unwrap();
    assert_eq!(session.compaction_count, 2);
}
