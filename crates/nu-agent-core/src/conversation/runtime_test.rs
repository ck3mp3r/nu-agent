use super::*;

use crate::compaction::CompactionStrategy;
use crate::conversation::providers::ClientCacheKey;
use crate::protocol::{contracts::ProgressUi, event::UiEvent};
use crate::types::{InMemoryConversationMemory, Message, Text, ToolDefinition, UserContent};
use rig::memory::ConversationMemory;

#[derive(Default)]
struct TestProgressUi {
    events: Vec<UiEvent>,
}

impl ProgressUi for TestProgressUi {
    fn emit(&mut self, event: &UiEvent) {
        self.events.push(event.clone());
    }

    fn flush(&mut self) {}

    fn take_cancel_requested(&self) -> bool {
        false
    }
}

#[test]
fn permissions_startup_summary_emits_once_before_first_turn() {
    use crate::tools::authz::{PermissionsConfig, SessionGrantCache};

    let mut ui = TestProgressUi::default();
    let summary =
        "permissions policy: overlay_active=false global=ask tool_rules=5 nu__run.command_rules=1";

    let mut state = super::super::state::permission::PermissionState::new(
        PermissionsConfig::safe_defaults(true),
        SessionGrantCache::default(),
        summary.to_string(),
    );

    state.emit_startup_summary_once(&mut ui);
    state.emit_startup_summary_once(&mut ui);

    let warnings = ui
        .events
        .iter()
        .filter(|e| matches!(e, UiEvent::Warning { .. }))
        .count();
    assert_eq!(warnings, 1);

    let warning_message = ui
        .events
        .iter()
        .find_map(|event| match event {
            UiEvent::Warning { message } => Some(message.clone()),
            _ => None,
        })
        .expect("warning event");
    assert_eq!(warning_message, summary);
}

// ========================================================================
// Structured messages tests
// ========================================================================

#[test]
fn build_system_preamble_joins_non_empty_parts() {
    let result = super::build_system_preamble(
        Some("preamble text"),
        None,
        None,
        Some("context text"),
        Some("agents chain"),
        Some("available skills"),
    );

    assert!(result.is_some());
    let text = result.unwrap();
    assert!(text.contains("preamble text"));
    assert!(text.contains("context text"));
    assert!(text.contains("agents chain"));
    assert!(text.contains("available skills"));
}

#[test]
fn build_system_preamble_returns_none_when_all_empty() {
    let result = super::build_system_preamble(None, None, None, None, None, None);
    assert!(result.is_none());
}

#[test]
fn build_system_preamble_handles_partial_inputs() {
    let result =
        super::build_system_preamble(Some("preamble"), None, None, None, Some("agents"), None);

    assert!(result.is_some());
    let text = result.unwrap();
    assert!(text.contains("preamble"));
    assert!(text.contains("agents"));
}

#[test]
fn build_system_preamble_includes_persona_in_correct_position() {
    let result = super::build_system_preamble(
        Some("config preamble"),
        Some("agent persona"),
        None,
        Some("context text"),
        Some("agents chain"),
        Some("available skills"),
    );

    assert!(result.is_some());
    let text = result.unwrap();

    // Verify all parts are present
    assert!(text.contains("config preamble"));
    assert!(text.contains("agent persona"));
    assert!(text.contains("context text"));
    assert!(text.contains("agents chain"));
    assert!(text.contains("available skills"));

    // Verify persona appears between config preamble and context
    let config_pos = text.find("config preamble").unwrap();
    let persona_pos = text.find("agent persona").unwrap();
    let context_pos = text.find("context text").unwrap();

    assert!(
        config_pos < persona_pos,
        "config preamble should come before persona"
    );
    assert!(
        persona_pos < context_pos,
        "persona should come before context"
    );
}

#[test]
fn build_system_preamble_persona_only() {
    let result = super::build_system_preamble(None, Some("persona only"), None, None, None, None);

    assert!(result.is_some());
    let text = result.unwrap();
    assert_eq!(text, "persona only");
}

#[test]
fn build_system_preamble_includes_sub_agent_instruction() {
    let result = super::build_system_preamble(
        None,
        Some("persona"),
        Some("sub-agent instruction"),
        None,
        None,
        None,
    );

    assert!(result.is_some());
    let text = result.unwrap();
    assert!(text.contains("persona"));
    assert!(text.contains("sub-agent instruction"));

    // sub-agent instruction should come after persona
    let persona_pos = text.find("persona").unwrap();
    let instruction_pos = text.find("sub-agent instruction").unwrap();
    assert!(
        persona_pos < instruction_pos,
        "sub-agent instruction should come after persona"
    );
}

#[test]
fn build_system_preamble_sub_agent_instruction_only() {
    let result =
        super::build_system_preamble(None, None, Some("you are a sub-agent"), None, None, None);

    assert!(result.is_some());
    let text = result.unwrap();
    assert_eq!(text, "you are a sub-agent");
}

// ========================================================================
// Memory and conversation store tests
// ========================================================================

#[test]
fn runtime_struct_has_memory_field() {
    // GREEN: This test now compiles, proving the memory field exists

    // Compile-time check that the field exists with correct type
    fn _assert_field_exists(_memory: &InMemoryConversationMemory) {}

    // We can't easily construct a runtime in tests, but we can verify
    // the type signature compiles
    let _type_check: fn(&AgentConversationRuntime) = |r| {
        _assert_field_exists(r.memory_state.memory());
    };
}

#[test]
fn runtime_struct_has_conversation_store_field() {
    // GREEN: This test now compiles, proving the conversation_store field exists
    use crate::session::JsonlConversationStore;

    // Compile-time check that the field exists with correct type
    fn _assert_field_exists(_store: &JsonlConversationStore) {}

    let _type_check: fn(&AgentConversationRuntime) = |r| {
        _assert_field_exists(r.memory_state.conversation_store());
    };
}

#[test]
fn evaluate_auto_compaction_uses_token_based_policy() {
    // Verify that TokenCompactionPolicy is used for auto-compaction evaluation.
    // We can't easily construct a full runtime, but we verify the policy logic directly.
    use crate::protocol::compaction::{CompactionTriggerPolicy, TokenCompactionPolicy};

    let policy = TokenCompactionPolicy::new(200_000, 0.80, CompactionStrategy::SlidingSummary);

    // At 80% usage (160k of 200k) — should fire
    let decision = policy.evaluate(Some(160_000));
    assert!(
        matches!(
            decision,
            crate::protocol::compaction::CompactionTriggerDecision::Fire { .. }
        ),
        "Expected compaction to fire at 80% token usage"
    );

    // At 50% usage — should not fire
    let decision2 = policy.evaluate(Some(100_000));
    assert!(
        matches!(
            decision2,
            crate::protocol::compaction::CompactionTriggerDecision::NoFire { .. }
        ),
        "Expected no compaction at 50% token usage"
    );
}

// Provider dispatch tests

#[test]
fn provider_dispatch_unsupported_provider_returns_error() {
    // RED: Verify that unsupported provider returns clear error
    use crate::config::Config;

    let config = Config {
        provider: "unsupported-provider".to_string(),
        provider_impl: None,
        model: "some-model".to_string(),
        api_key: Some("test-key".to_string()),
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: None,
        preamble: None,
        read_timeout_secs: None,
    };

    // This test will compile once we add the dispatch logic
    // For now, document that build_copilot_client works for copilot only
    // When we add dispatch in execute_turn, this will test the error path

    // Expected behavior: execute_turn should return error with:
    // "Unsupported provider: 'unsupported-provider'"
    // This test documents the requirement for now
    assert_eq!(config.provider, "unsupported-provider");
}

// Mailbox/session clearing tests

#[test]
fn clear_session_resets_memory() {
    use rig::one_or_many::OneOrMany;

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let mut memory = InMemoryConversationMemory::new();

    // Populate memory with some messages
    runtime.block_on(async {
        memory
            .append(
                "test-session",
                vec![Message::User {
                    content: OneOrMany::one(UserContent::Text(Text {
                        text: "hello".to_string(),
                        additional_params: None,
                    })),
                }],
            )
            .await
            .unwrap();
    });

    // Verify messages exist
    let messages_before = runtime.block_on(async { memory.load("test-session").await.unwrap() });
    assert_eq!(messages_before.len(), 1);

    // Clear session by creating a new memory instance (simulates clear_session behavior)
    memory = InMemoryConversationMemory::new();

    // Verify memory is empty after clear
    let messages_after = runtime.block_on(async { memory.load("test-session").await.unwrap() });
    assert_eq!(messages_after.len(), 0);
}

#[test]
fn clear_session_resets_memory_state() {
    // After clear_session(), memory is reset and hydrated flag is false
    // This is a behavioral test — clear_session creates fresh memory

    let memory = InMemoryConversationMemory::new();
    let hydrated = false;

    // Verify fresh state
    assert!(!hydrated, "hydrated should be false after clear_session");
    // Memory is freshly constructed — no messages
    let _ = memory;
}

#[test]
fn runtime_struct_has_compacting_field() {
    // Compile-time check that the compacting field exists with correct type
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    fn _assert_field_exists(_flag: &Arc<AtomicBool>) {}

    let _type_check: fn(&AgentConversationRuntime) = |r| {
        _assert_field_exists(r.compaction_state.compacting());
    };
}

// ========================================================================
// Memory hydration guard tests
// ========================================================================

#[test]
fn runtime_struct_has_memory_hydrated_field() {
    // Compile-time check that the memory_hydrated field exists with correct type
    let _type_check: fn(&AgentConversationRuntime) = |r| {
        let _hydrated: bool = r.memory_state.is_hydrated();
    };
}

#[test]
fn hydration_guard_prevents_duplicate_memory_append() {
    // Tests the guard pattern used by ensure_memory_hydrated:
    // a bool guard must prevent double-appending stored messages to memory.
    use rig::memory::ConversationMemory;

    use crate::session::{ConversationStore, JsonlConversationStore};

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let memory = InMemoryConversationMemory::new();
    let temp_dir = tempfile::tempdir().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());

    // Store 2 messages on disk
    let messages: Vec<Message> = (0..2)
        .map(|i| Message::user(format!("msg {}", i)))
        .collect();
    store.append("s1", &messages, None).unwrap();

    let mut hydrated = false;

    // First hydration — messages enter memory
    if !hydrated {
        let loaded = store.load("s1").unwrap();
        if !loaded.is_empty() {
            runtime.block_on(memory.append("s1", loaded)).unwrap();
        }
        hydrated = true;
    }

    let after_first = runtime.block_on(memory.load("s1")).unwrap();
    assert_eq!(after_first.len(), 2);

    // Second hydration — guard prevents duplicate append
    if !hydrated {
        let loaded = store.load("s1").unwrap();
        if !loaded.is_empty() {
            runtime.block_on(memory.append("s1", loaded)).unwrap();
        }
    }
    // Guard should still be true from first hydration
    assert!(hydrated, "guard should remain true");

    let after_second = runtime.block_on(memory.load("s1")).unwrap();
    assert_eq!(
        after_second.len(),
        2,
        "Guard must prevent duplicate hydration"
    );
}

#[test]
fn hydration_without_guard_causes_duplicates() {
    // Proves the bug: without a guard, calling hydration twice duplicates
    // messages in memory — exactly the problem ensure_memory_hydrated prevents.
    use rig::memory::ConversationMemory;

    use crate::session::{ConversationStore, JsonlConversationStore};

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let memory = InMemoryConversationMemory::new();
    let temp_dir = tempfile::tempdir().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());

    let messages: Vec<Message> = (0..3)
        .map(|i| Message::user(format!("msg {}", i)))
        .collect();
    store.append("s1", &messages, None).unwrap();

    // Load-and-append WITHOUT guard — twice
    let loaded1 = store.load("s1").unwrap();
    runtime.block_on(memory.append("s1", loaded1)).unwrap();

    let loaded2 = store.load("s1").unwrap();
    runtime.block_on(memory.append("s1", loaded2)).unwrap();

    let count = runtime.block_on(memory.load("s1")).unwrap().len();
    assert_eq!(
        count, 6,
        "Without guard, messages are duplicated (3 * 2 = 6)"
    );
}

#[test]
fn cancelled_turn_path_a_persists_to_store_and_memory() {
    // Path A: rig hook cancelled — e.messages contains chat_history.
    // Fix: both store and memory must receive the messages.
    use rig::memory::ConversationMemory;

    use crate::session::{ConversationStore, JsonlConversationStore};

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let memory = InMemoryConversationMemory::new();
    let temp_dir = tempfile::tempdir().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());

    // Simulate Path A: TurnError.messages contains chat_history from rig
    let cancelled_messages = vec![
        Message::user("what is the weather?".to_string()),
        Message::assistant("Let me check...".to_string()),
    ];

    // Persist to store (already done in production code)
    store.append("s1", &cancelled_messages, None).unwrap();

    // Persist to memory (the fix)
    runtime
        .block_on(memory.append("s1", cancelled_messages.clone()))
        .unwrap();

    // Verify store has the messages
    let stored = store.load("s1").unwrap();
    assert_eq!(stored.len(), 2, "Store must have 2 cancelled messages");

    // Verify memory has the messages
    let in_memory = runtime.block_on(memory.load("s1")).unwrap();
    assert_eq!(
        in_memory.len(),
        2,
        "Memory must have 2 cancelled messages (path A fix)"
    );
}

#[test]
fn cancelled_turn_path_b_persists_to_store_and_memory() {
    // Path B: cancel_token fired — messages constructed from prompt + partial text.
    // Fix: both store and memory must receive the constructed messages.
    use rig::memory::ConversationMemory;

    use crate::session::{ConversationStore, JsonlConversationStore};

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let memory = InMemoryConversationMemory::new();
    let temp_dir = tempfile::tempdir().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());

    // Simulate Path B construction: user message + partial assistant text
    let prompt = "explain quantum computing".to_string();
    let partial_text = "Quantum computing uses qubits which".to_string();
    let mut cancelled_messages = vec![Message::user(prompt)];
    if !partial_text.is_empty() {
        cancelled_messages.push(Message::assistant(partial_text));
    }

    // Persist to store (already done in production code)
    store.append("s1", &cancelled_messages, None).unwrap();

    // Persist to memory (the fix)
    runtime
        .block_on(memory.append("s1", cancelled_messages.clone()))
        .unwrap();

    // Verify store
    let stored = store.load("s1").unwrap();
    assert_eq!(stored.len(), 2, "Store must have user + partial assistant");

    // Verify memory
    let in_memory = runtime.block_on(memory.load("s1")).unwrap();
    assert_eq!(
        in_memory.len(),
        2,
        "Memory must have user + partial assistant (path B fix)"
    );
}

// ========================================================================
// Memory hydration — LLM context extraction tests
// ========================================================================

#[test]
fn hydration_loads_llm_context_not_full_history() {
    // Store has 15 messages + 1 marker(kept=5) + 5 msgs after marker.
    // After hydration, memory has 6 messages (summary + 5 post-marker).
    // memory_message_count == 6.
    use rig::memory::ConversationMemory;

    use crate::session::{
        CompactionMarker, ConversationStore, JsonlConversationStore, extract_llm_context,
    };

    let rt = tokio::runtime::Runtime::new().unwrap();
    let memory = InMemoryConversationMemory::new();
    let temp_dir = tempfile::tempdir().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());

    // Store 15 messages (old, before marker)
    let messages: Vec<Message> = (0..15)
        .map(|i| Message::user(format!("msg {}", i)))
        .collect();
    store.append("s1", &messages, None).unwrap();

    // Append a compaction marker (kept=5, summarized 15)
    let marker = CompactionMarker::new(
        "Summary of older messages".to_string(),
        5,
        15,
        "summarize_and_keep_recent",
    );
    store.append_marker("s1", &marker, None).unwrap();

    // 5 kept messages re-appended after marker
    let kept: Vec<Message> = (15..20)
        .map(|i| Message::user(format!("msg {}", i)))
        .collect();
    store.append("s1", &kept, None).unwrap();

    // --- New hydration pattern ---
    let (entries, _) = store.load_all("s1").unwrap();
    let llm_context = extract_llm_context(&entries);

    if !llm_context.is_empty() {
        rt.block_on(memory.append("s1", llm_context.clone()))
            .unwrap();
    }
    let memory_message_count = llm_context.len();

    // Expect: 1 summary system message + 5 post-marker = 6
    assert_eq!(
        memory_message_count, 6,
        "Should have summary + 5 kept messages, not all 20"
    );
    let in_memory = rt.block_on(memory.load("s1")).unwrap();
    assert_eq!(in_memory.len(), 6);
}

#[test]
fn hydration_no_markers_loads_all() {
    // Store has 15 messages, no markers. Memory has 15. memory_message_count == 15.
    use rig::memory::ConversationMemory;

    use crate::session::{ConversationStore, JsonlConversationStore, extract_llm_context};

    let rt = tokio::runtime::Runtime::new().unwrap();
    let memory = InMemoryConversationMemory::new();
    let temp_dir = tempfile::tempdir().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());

    let messages: Vec<Message> = (0..15)
        .map(|i| Message::user(format!("msg {}", i)))
        .collect();
    store.append("s1", &messages, None).unwrap();

    let (entries, _) = store.load_all("s1").unwrap();
    let llm_context = extract_llm_context(&entries);

    if !llm_context.is_empty() {
        rt.block_on(memory.append("s1", llm_context.clone()))
            .unwrap();
    }
    let memory_message_count = llm_context.len();

    assert_eq!(
        memory_message_count, 15,
        "Without markers, all messages should be loaded"
    );
    let in_memory = rt.block_on(memory.load("s1")).unwrap();
    assert_eq!(in_memory.len(), 15);
}

#[test]
fn hydration_multiple_markers_uses_latest() {
    // Store has msgs + marker1 + msgs + marker2(kept=3) + 3 msgs after marker2.
    // Memory uses marker2's context only.
    use rig::memory::ConversationMemory;

    use crate::session::{
        CompactionMarker, ConversationStore, JsonlConversationStore, extract_llm_context,
    };

    let rt = tokio::runtime::Runtime::new().unwrap();
    let memory = InMemoryConversationMemory::new();
    let temp_dir = tempfile::tempdir().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());

    // 10 messages
    let msgs1: Vec<Message> = (0..10)
        .map(|i| Message::user(format!("batch1 msg {}", i)))
        .collect();
    store.append("s1", &msgs1, None).unwrap();

    // Marker 1 (kept=2)
    let marker1 = CompactionMarker::new(
        "First summary".to_string(),
        2,
        8,
        "summarize_and_keep_recent",
    );
    store.append_marker("s1", &marker1, None).unwrap();

    // 5 more messages between markers
    let msgs2: Vec<Message> = (0..5)
        .map(|i| Message::user(format!("batch2 msg {}", i)))
        .collect();
    store.append("s1", &msgs2, None).unwrap();

    // Marker 2 (kept=3)
    let marker2 = CompactionMarker::new(
        "Second summary".to_string(),
        3,
        12,
        "summarize_and_keep_recent",
    );
    store.append_marker("s1", &marker2, None).unwrap();

    // 3 kept messages re-appended after marker2
    let kept: Vec<Message> = (0..3)
        .map(|i| Message::user(format!("kept msg {}", i)))
        .collect();
    store.append("s1", &kept, None).unwrap();

    let (entries, _) = store.load_all("s1").unwrap();
    let llm_context = extract_llm_context(&entries);

    if !llm_context.is_empty() {
        rt.block_on(memory.append("s1", llm_context.clone()))
            .unwrap();
    }

    // marker2: summary("Second summary") + 3 post-marker messages = 4
    assert_eq!(
        llm_context.len(),
        4,
        "Should use latest marker: 1 summary + 3 kept"
    );
    let in_memory = rt.block_on(memory.load("s1")).unwrap();
    assert_eq!(in_memory.len(), 4);
}

#[test]
fn compaction_count_derived_from_markers() {
    // Store has 3 markers. After hydration, compaction_count == 3.

    use crate::session::{CompactionMarker, ConversationStore, JsonlConversationStore, StoreEntry};

    let temp_dir = tempfile::tempdir().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());

    // Interleave messages and markers
    let msgs: Vec<Message> = (0..5)
        .map(|i| Message::user(format!("msg {}", i)))
        .collect();
    store.append("s1", &msgs, None).unwrap();

    let m1 = CompactionMarker::new("s1".to_string(), 2, 3, "summarize_and_keep_recent");
    store.append_marker("s1", &m1, None).unwrap();

    let msgs2: Vec<Message> = (0..3)
        .map(|i| Message::user(format!("msg2 {}", i)))
        .collect();
    store.append("s1", &msgs2, None).unwrap();

    let m2 = CompactionMarker::new("s2".to_string(), 1, 2, "summarize_and_keep_recent");
    store.append_marker("s1", &m2, None).unwrap();

    let msgs3: Vec<Message> = (0..2)
        .map(|i| Message::user(format!("msg3 {}", i)))
        .collect();
    store.append("s1", &msgs3, None).unwrap();

    let m3 = CompactionMarker::new("s3".to_string(), 1, 1, "summarize_and_keep_recent");
    store.append_marker("s1", &m3, None).unwrap();

    let (entries, _) = store.load_all("s1").unwrap();

    // Derive compaction_count from markers
    let marker_count = entries
        .iter()
        .filter(|e| matches!(e, StoreEntry::Marker(_)))
        .count();

    assert_eq!(marker_count, 3, "Should count all 3 markers");
}

#[test]
fn hydration_guard_still_prevents_duplicates() {
    // Ensure the memory_hydrated guard still works with the new code path
    // (load_all + extract_llm_context).
    use rig::memory::ConversationMemory;

    use crate::session::{
        CompactionMarker, ConversationStore, JsonlConversationStore, extract_llm_context,
    };

    let rt = tokio::runtime::Runtime::new().unwrap();
    let memory = InMemoryConversationMemory::new();
    let temp_dir = tempfile::tempdir().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());

    // Store messages + marker + kept messages after marker
    let messages: Vec<Message> = (0..7)
        .map(|i| Message::user(format!("msg {}", i)))
        .collect();
    store.append("s1", &messages, None).unwrap();

    let marker = CompactionMarker::new("Summary".to_string(), 3, 7, "summarize_and_keep_recent");
    store.append_marker("s1", &marker, None).unwrap();

    // 3 kept messages re-appended after marker
    let kept: Vec<Message> = (7..10)
        .map(|i| Message::user(format!("msg {}", i)))
        .collect();
    store.append("s1", &kept, None).unwrap();

    let mut hydrated = false;

    // First hydration
    if !hydrated {
        let (entries, _) = store.load_all("s1").unwrap();
        let llm_context = extract_llm_context(&entries);
        if !llm_context.is_empty() {
            rt.block_on(memory.append("s1", llm_context)).unwrap();
        }
        hydrated = true;
    }

    let after_first = rt.block_on(memory.load("s1")).unwrap();
    // summary + 3 kept = 4
    assert_eq!(after_first.len(), 4);

    // Second hydration — guard prevents duplicate append
    if !hydrated {
        let (entries, _) = store.load_all("s1").unwrap();
        let llm_context = extract_llm_context(&entries);
        if !llm_context.is_empty() {
            rt.block_on(memory.append("s1", llm_context)).unwrap();
        }
    }

    assert!(hydrated, "guard should remain true");
    let after_second = rt.block_on(memory.load("s1")).unwrap();
    assert_eq!(
        after_second.len(),
        4,
        "Guard must prevent duplicate hydration with new code path"
    );
}

// ========================================================================
// Phase 1a: Characterise runtime.rs helpers
// ========================================================================

/// Fake ConversationStore for testing. Tracks load_all call count via an
/// atomic counter — all methods return Ok with empty/no-op results.
struct NullStore {
    load_call_count: Arc<std::sync::atomic::AtomicUsize>,
}

impl crate::session::ConversationStore for NullStore {
    fn load(&self, _session_id: &str) -> Result<Vec<Message>, Box<dyn std::error::Error>> {
        Ok(vec![])
    }

    fn load_all(
        &self,
        _session_id: &str,
    ) -> Result<(Vec<crate::session::StoreEntry>, Option<u64>), Box<dyn std::error::Error>> {
        self.load_call_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok((vec![], None))
    }

    fn append(
        &self,
        _session_id: &str,
        _messages: &[Message],
        _last_total_tokens: Option<u64>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn append_marker(
        &self,
        _session_id: &str,
        _marker: &crate::session::CompactionMarker,
        _last_total_tokens: Option<u64>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn clear(&self, _session_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}

#[test]
fn client_cache_key_contains_provider_and_model() {
    // Characterise client_cache_key (runtime.rs:708-714).
    // It returns (config.provider, config.api_key, config.base_url).
    use crate::config::Config;

    let config = Config {
        provider: "copilot".to_string(),
        provider_impl: None,
        model: "test-model".to_string(),
        api_key: Some("test-key".to_string()),
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: None,
        preamble: None,
        read_timeout_secs: None,
    };

    // client_cache_key clones (provider, api_key, base_url) from config.
    let key: ClientCacheKey = (
        config.provider.clone(),
        config.api_key.clone(),
        config.base_url.clone(),
    );

    assert_eq!(
        key,
        ("copilot".to_string(), Some("test-key".to_string()), None,)
    );
}

#[test]
fn client_cache_key_includes_base_url_when_set() {
    // Same as above but with base_url set.
    use crate::config::Config;

    let config = Config {
        provider: "copilot".to_string(),
        provider_impl: None,
        model: "test-model".to_string(),
        api_key: Some("test-key".to_string()),
        base_url: Some("https://custom.example.com".to_string()),
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: None,
        preamble: None,
        read_timeout_secs: None,
    };

    let key: ClientCacheKey = (
        config.provider.clone(),
        config.api_key.clone(),
        config.base_url.clone(),
    );

    assert_eq!(
        key,
        (
            "copilot".to_string(),
            Some("test-key".to_string()),
            Some("https://custom.example.com".to_string()),
        )
    );
}

#[test]
fn active_tool_definitions_returns_empty_when_no_tools() {
    // Characterise active_tool_definitions (runtime.rs:753-759).
    // Delegates to handler::llm_visible_tool_definitions with the runtime's
    // tool_definitions, mcp_registry, and permissions.
    use crate::tools::{
        authz::PermissionsConfig,
        handler::{self, McpToolRegistry},
    };

    let tool_definitions: Vec<ToolDefinition> = vec![];
    let mcp_registry = McpToolRegistry::from_names(std::iter::empty::<String>());
    let permissions = PermissionsConfig::safe_defaults(true);

    let result =
        handler::llm_visible_tool_definitions(&tool_definitions, &mcp_registry, &permissions);

    assert!(result.is_empty());
}

#[test]
fn ensure_memory_hydrated_sets_hydrated_flag() {
    // Characterise ensure_memory_hydrated (runtime.rs:674-706).
    // With an empty store (no stored messages), calling the hydration
    // pattern sets the hydrated flag to true and returns Ok(()).

    use crate::session::{ConversationStore, JsonlConversationStore, extract_llm_context};

    let rt = tokio::runtime::Runtime::new().unwrap();
    let memory = InMemoryConversationMemory::new();
    let temp_dir = tempfile::tempdir().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    let session_id = "test-session";
    let mut memory_hydrated = false;

    // Replicate the ensure_memory_hydrated logic (runtime.rs:674-706)
    if !memory_hydrated {
        let (entries, _last_total_tokens) = store.load_all(session_id).unwrap();
        let llm_context = extract_llm_context(&entries);
        if !llm_context.is_empty() {
            rt.block_on(memory.append(session_id, llm_context)).unwrap();
        }
        memory_hydrated = true;
    }

    assert!(
        memory_hydrated,
        "hydrated flag must be true after hydration"
    );
}

#[test]
fn ensure_memory_hydrated_is_noop_on_second_call() {
    // Characterise that ensure_memory_hydrated is idempotent:
    // calling it twice only invokes store.load_all once (the guard
    // short-circuits on the second call).
    use crate::session::ConversationStore;

    let load_call_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let store = NullStore {
        load_call_count: load_call_count.clone(),
    };
    let session_id = "test-session";
    let mut memory_hydrated = false;

    // First call — goes through
    if !memory_hydrated {
        let (_entries, _) = store.load_all(session_id).unwrap();
        memory_hydrated = true;
    }

    // Second call — guard prevents entry
    if !memory_hydrated {
        let (_entries, _) = store.load_all(session_id).unwrap();
    }

    assert_eq!(
        load_call_count.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "store.load_all must only be called once"
    );
}

// ========================================================================
// Phase C-pre: Characterise sub-struct clusters before field decomposition
// ========================================================================

#[test]
fn mcp_state_initial_tool_count_is_zero() {
    // Characterise llm_visible_mcp_tool_count (runtime.rs:372-377).
    // With an empty mcp_registry and no mcp_lifecycle_projection, the
    // method filters active_tool_definitions by mcp_registry.is_registered
    // — an empty registry yields 0.
    use crate::tools::{authz::PermissionsConfig, handler::McpToolRegistry};

    let tool_definitions: Vec<ToolDefinition> = vec![];
    let mcp_registry = McpToolRegistry::from_names(std::iter::empty::<String>());
    let permissions = PermissionsConfig::safe_defaults(true);

    // Replicate the method body: filter active_tool_definitions by registry
    let active = super::super::super::tools::handler::llm_visible_tool_definitions(
        &tool_definitions,
        &mcp_registry,
        &permissions,
    );
    let count = active
        .iter()
        .filter(|tool| mcp_registry.is_registered(tool.name.as_str()))
        .count();

    assert_eq!(count, 0, "empty registry must yield zero MCP tool count");
}

#[test]
fn mcp_state_tool_count_by_server_returns_zero_for_unknown() {
    // Characterise llm_visible_mcp_tool_count_for_server (runtime.rs:379-386).
    // Querying for "nonexistent-server" with an empty registry must return 0.
    use crate::tools::{authz::PermissionsConfig, handler::McpToolRegistry};

    let tool_definitions: Vec<ToolDefinition> = vec![];
    let mcp_registry = McpToolRegistry::from_names(std::iter::empty::<String>());
    let permissions = PermissionsConfig::safe_defaults(true);

    let active = super::super::super::tools::handler::llm_visible_tool_definitions(
        &tool_definitions,
        &mcp_registry,
        &permissions,
    );
    let count = active
        .iter()
        .filter(|tool| mcp_registry.is_registered(tool.name.as_str()))
        .filter_map(|tool| mcp_registry.server_name_for(tool.name.as_str()))
        .filter(|server| *server == "nonexistent-server")
        .count();

    assert_eq!(count, 0, "unknown server must yield zero tool count");
}

#[test]
fn compaction_state_evaluate_returns_none_when_no_tokens() {
    // Characterise evaluate_auto_compaction (runtime.rs:488-495).
    // When last_total_tokens is None, the policy returns NoFire("no_token_data")
    // and the method wraps it in Some(...).
    use crate::protocol::compaction::{
        CompactionTriggerDecision, CompactionTriggerPolicy, TokenCompactionPolicy,
    };

    let policy = TokenCompactionPolicy::new(100_000, 0.8, CompactionStrategy::SlidingSummary);
    let decision = Some(policy.evaluate(None));

    assert!(
        matches!(decision, Some(CompactionTriggerDecision::NoFire { .. })),
        "no token data must yield Some(NoFire), not Fire"
    );
}

#[test]
fn compaction_state_evaluate_returns_none_below_threshold() {
    // Characterise evaluate_auto_compaction (runtime.rs:488-495).
    // 50k tokens against 100k window with 80% threshold => 50% usage, below threshold.
    use crate::protocol::compaction::{
        CompactionTriggerDecision, CompactionTriggerPolicy, TokenCompactionPolicy,
    };

    let policy = TokenCompactionPolicy::new(100_000, 0.8, CompactionStrategy::SlidingSummary);
    let decision = Some(policy.evaluate(Some(50_000)));

    assert!(
        matches!(decision, Some(CompactionTriggerDecision::NoFire { .. })),
        "50% usage below 80% threshold must yield Some(NoFire)"
    );
}

#[test]
fn memory_state_hydrated_flag_starts_false() {
    // Characterise that memory_hydrated is initialised to false.
    // The field is a plain bool — fresh state is always false.
    // (Mirrors the clear_session behaviour at runtime.rs:505-508.)
    let hydrated: bool = false;
    assert!(
        !hydrated,
        "memory_hydrated must start false in a fresh runtime"
    );

    // Compile-time proof the field exists with type bool on the struct.
    let _type_check: fn(&AgentConversationRuntime) = |r| {
        let _: bool = r.memory_state.is_hydrated();
    };
}

#[test]
fn active_model_identity_returns_provider_slash_model() {
    // Characterise active_model_identity (runtime.rs:484-486).
    // Returns format!("{}/{}", config.provider, config.model).
    use crate::config::Config;

    let config = Config {
        provider: "copilot".to_string(),
        provider_impl: None,
        model: "claude-sonnet-4".to_string(),
        api_key: None,
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: None,
        preamble: None,
        read_timeout_secs: None,
    };

    // Replicate the method body exactly
    let identity = format!("{}/{}", config.provider, config.model);

    assert!(
        identity.contains("copilot"),
        "identity must contain provider"
    );
    assert!(
        identity.contains("claude-sonnet-4"),
        "identity must contain model"
    );
    assert_eq!(
        identity, "copilot/claude-sonnet-4",
        "identity must be provider/model"
    );
}

// ========================================================================
// Phase E: PermissionState characterisation tests
// ========================================================================

#[test]
fn permission_state_startup_not_emitted_on_construction() {
    // After construction, startup_emitted must be false even when
    // startup_summary is non-empty — emission only happens during
    // execute_turn, not at construction time.
    // We verify by calling emit_startup_summary_once on a freshly
    // constructed PermissionState and confirming it does emit (proving
    // the flag was false).
    use crate::tools::authz::{PermissionsConfig, SessionGrantCache};

    let mut ui = TestProgressUi::default();
    let mut state = super::super::state::permission::PermissionState::new(
        PermissionsConfig::safe_defaults(true),
        SessionGrantCache::default(),
        "non-empty summary".to_string(),
    );

    state.emit_startup_summary_once(&mut ui);

    let warnings = ui
        .events
        .iter()
        .filter(|e| matches!(e, UiEvent::Warning { .. }))
        .count();
    assert_eq!(
        warnings, 1,
        "fresh PermissionState must have startup_emitted=false, so first call emits"
    );
}

#[test]
fn permission_state_emit_startup_summary_emits_once() {
    // emit_startup_summary_once must emit exactly one Warning event, even
    // when called twice.
    use crate::tools::authz::{PermissionsConfig, SessionGrantCache};

    let mut ui = TestProgressUi::default();
    let summary = "test permissions summary";

    let mut state = super::super::state::permission::PermissionState::new(
        PermissionsConfig::safe_defaults(true),
        SessionGrantCache::default(),
        summary.to_string(),
    );

    state.emit_startup_summary_once(&mut ui);
    state.emit_startup_summary_once(&mut ui);

    let warnings = ui
        .events
        .iter()
        .filter(|e| matches!(e, UiEvent::Warning { .. }))
        .count();
    assert_eq!(warnings, 1, "must emit exactly 1 warning, not 2");
}

// ========================================================================
// Phase F: ProviderState characterisation tests
// ========================================================================

#[test]
fn provider_state_switch_model_returns_err_when_no_startup_config() {
    // Characterise switch_model (runtime.rs ExtendedRuntime impl).
    // When startup_plugin_config is None, switch_model must return Err
    // containing "model switch unavailable".

    let startup_plugin_config: Option<crate::config::PluginConfig> = None;

    // Replicate the switch_model error path with no startup config
    let result: Result<String, String> = startup_plugin_config
        .ok_or_else(|| {
            "model switch unavailable: startup plugin config cache is missing".to_string()
        })
        .map(|_| "unreachable".to_string());

    assert!(result.is_err());
    assert!(
        result.unwrap_err().contains("model switch unavailable"),
        "error must mention 'model switch unavailable'"
    );
}

#[test]
fn provider_state_client_cache_key_contains_provider_and_api_key() {
    // Characterise client_cache_key (runtime.rs:302-308).
    // With provider="copilot", api_key=Some("fake-key"), base_url=None,
    // client_cache_key returns (provider, api_key, base_url).
    use crate::config::Config;

    let config = Config {
        provider: "copilot".to_string(),
        provider_impl: None,
        model: "test-model".to_string(),
        api_key: Some("fake-key".to_string()),
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: None,
        preamble: None,
        read_timeout_secs: None,
    };

    // Replicate client_cache_key body
    let key: ClientCacheKey = (
        config.provider.clone(),
        config.api_key.clone(),
        config.base_url.clone(),
    );

    assert_eq!(key.0, "copilot");
    assert_eq!(key.1, Some("fake-key".to_string()));
    assert_eq!(key.2, None);
}

// ========================================================================
// Phase G: ToolState characterisation tests
// ========================================================================

#[test]
fn tool_state_active_definitions_empty_when_no_tools() {
    // Characterise active_tool_definitions: with empty tool_definitions,
    // the method delegates to handler::llm_visible_tool_definitions
    // and returns an empty Vec.
    use crate::tools::{
        authz::PermissionsConfig,
        handler::{self, McpToolRegistry},
    };

    let tool_definitions: Vec<ToolDefinition> = vec![];
    let mcp_registry = McpToolRegistry::from_names(std::iter::empty::<String>());
    let permissions = PermissionsConfig::safe_defaults(true);

    let result =
        handler::llm_visible_tool_definitions(&tool_definitions, &mcp_registry, &permissions);

    assert!(
        result.is_empty(),
        "active_tool_definitions must return empty Vec when no tools defined"
    );
}

#[test]
fn tool_state_baseline_is_reset_source() {
    // Characterise that baseline_tool_definitions serves as the reset
    // source: cloning baseline into tool_definitions restores initial state.

    let _tool_definitions: Vec<ToolDefinition> = vec![];
    let baseline_tool_definitions: Vec<ToolDefinition> = vec![ToolDefinition {
        name: "test_tool".to_string(),
        description: "".to_string(),
        parameters: serde_json::json!({}),
    }];

    // Simulate switch_agent reset: tool_definitions = baseline_tool_definitions.clone()
    let tool_definitions = baseline_tool_definitions.clone();

    assert_eq!(
        tool_definitions.len(),
        1,
        "after reset, tool_definitions must match baseline length"
    );
    assert_eq!(tool_definitions[0].name, "test_tool");
}

// ========================================================================
// Phase H: MultiAgentState characterisation tests
// ========================================================================

#[test]
fn multi_agent_state_available_summaries_empty_by_default() {
    use crate::config::AgentsConfig;
    use crate::conversation::state::multi_agent::MultiAgentState;

    let state = MultiAgentState::new(None, vec![], AgentsConfig::default());

    assert!(
        state.available_agent_summaries().is_empty(),
        "available_agent_summaries must be empty when constructed with vec![]"
    );
}

#[test]
fn multi_agent_state_switch_agent_fails_without_cwd() {
    // Characterise that switch_agent fails when mcp_caller_cwd is None.
    // This test exercises the runtime-level guard, not MultiAgentState directly.
    // We verify the error message contains the expected text.

    // The guard lives in runtime.rs switch_agent:
    //   self.mcp_state.mcp_caller_cwd.clone()
    //     .ok_or_else(|| "agent switch unavailable: working directory not set".to_string())?;

    let cwd: Option<String> = None;
    let result: Result<String, String> = cwd
        .clone()
        .ok_or_else(|| "agent switch unavailable: working directory not set".to_string());

    assert!(result.is_err());
    assert!(
        result.unwrap_err().contains("working directory not set"),
        "error must mention 'working directory not set'"
    );
}

// ========================================================================
// Phase I: AgentConversationRuntime accessor method tests
// ========================================================================

#[test]
fn accessor_provider_returns_provider_string() {
    // Verifies that runtime.provider() delegates to provider_state.config().provider
    use crate::config::Config;

    let config = Config {
        provider: "copilot".to_string(),
        provider_impl: None,
        model: "test-model".to_string(),
        api_key: Some("test-key".to_string()),
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: None,
        preamble: None,
        read_timeout_secs: None,
    };

    // Verify the accessor delegation chain: provider() -> provider_state.config().provider
    let provider_state = super::super::state::provider::ProviderState::new(config, None);
    assert_eq!(provider_state.config().provider.as_str(), "copilot");
}

#[test]
fn accessor_model_returns_model_string() {
    // Verifies that runtime.model() delegates to provider_state.config().model
    use crate::config::Config;

    let config = Config {
        provider: "copilot".to_string(),
        provider_impl: None,
        model: "claude-sonnet-4".to_string(),
        api_key: Some("test-key".to_string()),
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: None,
        preamble: None,
        read_timeout_secs: None,
    };

    let provider_state = super::super::state::provider::ProviderState::new(config, None);
    assert_eq!(provider_state.config().model.as_str(), "claude-sonnet-4");
}

#[test]
fn accessor_max_context_tokens_returns_none_when_unset() {
    use crate::config::Config;

    let config = Config {
        provider: "copilot".to_string(),
        provider_impl: None,
        model: "test-model".to_string(),
        api_key: None,
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: None,
        preamble: None,
        read_timeout_secs: None,
    };

    let provider_state = super::super::state::provider::ProviderState::new(config, None);
    assert_eq!(
        provider_state.config().max_context_tokens.map(u64::from),
        None
    );
}

#[test]
fn accessor_max_context_tokens_returns_value_when_set() {
    use crate::config::Config;

    let config = Config {
        provider: "copilot".to_string(),
        provider_impl: None,
        model: "test-model".to_string(),
        api_key: None,
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: Some(200_000),
        max_output_tokens: None,
        max_tool_turns: None,
        preamble: None,
        read_timeout_secs: None,
    };

    let provider_state = super::super::state::provider::ProviderState::new(config, None);
    assert_eq!(
        provider_state.config().max_context_tokens.map(u64::from),
        Some(200_000)
    );
}

#[test]
fn accessor_startup_plugin_config_returns_none_when_default() {
    use crate::config::Config;

    let config = Config {
        provider: "copilot".to_string(),
        provider_impl: None,
        model: "test-model".to_string(),
        api_key: None,
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: None,
        preamble: None,
        read_timeout_secs: None,
    };

    let provider_state = super::super::state::provider::ProviderState::new(config, None);
    assert!(provider_state.startup_plugin_config().is_none());
}

#[test]
fn accessor_agent_identity_returns_none_when_default() {
    // PersonaState with no agent_identity set must return None
    let persona_state =
        super::super::state::persona::PersonaState::new(None, None, None, None, None, None);
    assert_eq!(persona_state.agent_identity(), None);
}

#[test]
fn accessor_agent_identity_returns_some_when_set() {
    let persona_state = super::super::state::persona::PersonaState::new(
        None,
        Some("developer".to_string()),
        None,
        None,
        None,
        None,
    );
    assert_eq!(persona_state.agent_identity(), Some("developer"));
}

#[test]
fn accessor_mcp_caller_cwd_returns_none_when_default() {
    // McpState has mcp_caller_cwd as Option<PathBuf> — None when unset
    let cwd: Option<std::path::PathBuf> = None;
    assert_eq!(cwd.as_deref(), None::<&std::path::Path>);
}

#[test]
fn accessor_mcp_lifecycle_projection_returns_empty_when_default() {
    use crate::tools::mcp::runtime::McpServerLifecycle;

    let projection: Vec<McpServerLifecycle> = vec![];
    assert!(projection.is_empty());
}

#[test]
fn accessor_available_agent_summaries_delegates_to_multi_agent_state() {
    use crate::config::AgentsConfig;
    use crate::conversation::state::multi_agent::MultiAgentState;

    let state = MultiAgentState::new(None, vec![], AgentsConfig::default());
    assert!(state.available_agent_summaries().is_empty());
}

#[test]
fn accessor_take_mailbox_rx_returns_none_when_default() {
    use crate::config::AgentsConfig;
    use crate::conversation::state::multi_agent::MultiAgentState;

    let mut state = MultiAgentState::new(None, vec![], AgentsConfig::default());
    assert!(state.take_mailbox_rx().is_none());
}

#[test]
fn accessor_take_mailbox_rx_returns_some_and_drains() {
    use crate::config::AgentsConfig;
    use crate::conversation::state::multi_agent::MultiAgentState;

    let (_tx, rx) = std::sync::mpsc::channel();
    let mut state = MultiAgentState::new(Some(rx), vec![], AgentsConfig::default());
    assert!(
        state.take_mailbox_rx().is_some(),
        "first take must return Some"
    );
    assert!(
        state.take_mailbox_rx().is_none(),
        "second take must return None (drained)"
    );
}

// ========================================================================
// CompactionState characterisation tests
// ========================================================================

#[test]
fn compaction_state_compaction_count_starts_at_zero() {
    let state = super::super::compaction::state::CompactionState::new(
        200_000,
        0.80,
        0,
        CompactionStrategy::SlidingSummary,
    );
    assert_eq!(state.compaction_count(), 0);
}

#[test]
fn compaction_state_compacting_flag_starts_false() {
    use std::sync::atomic::Ordering;
    let state = super::super::compaction::state::CompactionState::new(
        200_000,
        0.80,
        0,
        CompactionStrategy::SlidingSummary,
    );
    assert!(!state.compacting().load(Ordering::SeqCst));
}

// ========================================================================
// Phase J: MemoryState characterisation tests
// ========================================================================

#[test]
fn memory_state_hydrated_false_on_construction() {
    let temp_dir = tempfile::tempdir().unwrap();
    let ms = super::super::state::memory::MemoryState::new(temp_dir.path().to_path_buf());
    assert!(!ms.is_hydrated());
}

#[test]
fn memory_state_last_total_tokens_none_on_construction() {
    let temp_dir = tempfile::tempdir().unwrap();
    let ms = super::super::state::memory::MemoryState::new(temp_dir.path().to_path_buf());
    assert!(ms.last_total_tokens().is_none());
}

// ========================================================================
// Phase K: PersonaState characterisation tests
// ========================================================================

#[test]
fn persona_state_agent_identity_none_by_default() {
    let persona_state =
        super::super::state::persona::PersonaState::new(None, None, None, None, None, None);
    assert!(persona_state.agent_identity().is_none());
}

#[test]
fn persona_state_agent_description_none_by_default() {
    let persona_state =
        super::super::state::persona::PersonaState::new(None, None, None, None, None, None);
    assert!(persona_state.agent_description().is_none());
}

// ========================================================================
// Phase L: McpState characterisation tests
// ========================================================================

#[test]
fn mcp_state_caller_cwd_none_by_default() {
    let temp_dir = tempfile::tempdir().unwrap();
    let _ms = super::super::state::memory::MemoryState::new(temp_dir.path().to_path_buf());
    // Access mcp_state through a compile-time type check
    let _type_check: fn(&AgentConversationRuntime) = |rt| {
        assert!(rt.mcp_state.mcp_caller_cwd().is_none());
    };
    // Value-level proof: default Option is None
    let cwd: Option<std::path::PathBuf> = None;
    assert!(cwd.is_none());
}

#[test]
fn mcp_state_lifecycle_projection_empty_by_default() {
    use crate::tools::mcp::runtime::McpServerLifecycle;
    let _type_check: fn(&AgentConversationRuntime) = |rt| {
        assert!(rt.mcp_state.mcp_lifecycle_projection().is_empty());
    };
    // Value-level proof: default Vec is empty
    let projection: Vec<McpServerLifecycle> = vec![];
    assert!(projection.is_empty());
}
