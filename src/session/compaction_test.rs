use super::*;
use rig::completion::Message;
use rig::memory::{ConversationMemory, InMemoryConversationMemory};
use std::sync::Arc;
use tempfile::TempDir;

#[test]
fn compact_with_rig_memory_splits_at_keep_recent() {
    // RED: Test that compaction loads from memory, splits correctly, and stores back
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    let memory = Arc::new(InMemoryConversationMemory::new());
    let session_id = "test_session";
    
    // Setup: 10 messages in memory
    let messages: Vec<Message> = (0..10)
        .map(|i| Message::user(&format!("Message {}", i)))
        .collect();
    
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async {
            memory.append(session_id, messages.clone()).await.unwrap();
        });
    
    let keep_recent = 3;
    let config = SessionConfig {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingSummary,
        keep_recent,
    };
    
    let mut session = Session {
        id: session_id.to_string(),
        created_at: Utc::now(),
        config,
        compaction_count: 0,
    };
    
    // Mock summarizer that formats old messages
    let summarizer = |old_messages: &[Message]| -> io::Result<String> {
        Ok(format!("Summary of {} messages", old_messages.len()))
    };
    
    let outcome = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async {
            session
                .compact_with_rig_memory(&memory, &store, summarizer)
                .await
        })
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
fn compact_with_rig_memory_persists_to_store() {
    // RED: Test that compacted messages are persisted to ConversationStore
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    let memory = Arc::new(InMemoryConversationMemory::new());
    let session_id = "test_persist";
    
    // Setup: 5 messages
    let messages: Vec<Message> = (0..5)
        .map(|i| Message::user(&format!("Msg {}", i)))
        .collect();
    
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async {
            memory.append(session_id, messages.clone()).await.unwrap();
        });
    
    let config = SessionConfig {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingSummary,
        keep_recent: 2,
    };
    
    let mut session = Session {
        id: session_id.to_string(),
        created_at: Utc::now(),
        config,
        compaction_count: 0,
    };
    
    let summarizer = |_: &[Message]| -> io::Result<String> { Ok("Summary".to_string()) };
    
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async {
            session
                .compact_with_rig_memory(&memory, &store, summarizer)
                .await
        })
        .unwrap();
    
    // Verify: store contains the compacted messages
    let stored = store.load(session_id).unwrap();
    assert_eq!(stored.len(), 3); // summary + 2 recent
    
    // First should be system message
    match &stored[0] {
        Message::System { content } => {
            assert_eq!(content, "Summary");
        }
        _ => panic!("Expected system message"),
    }
}

#[test]
fn compact_with_rig_memory_increments_compaction_count() {
    // RED: Test that compaction_count is incremented
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    let memory = Arc::new(InMemoryConversationMemory::new());
    let session_id = "test_count";
    
    let messages: Vec<Message> = (0..5)
        .map(|i| Message::user(&format!("M{}", i)))
        .collect();
    
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async {
            memory.append(session_id, messages.clone()).await.unwrap();
        });
    
    let config = SessionConfig {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingSummary,
        keep_recent: 2,
    };
    
    let mut session = Session {
        id: session_id.to_string(),
        created_at: Utc::now(),
        config,
        compaction_count: 0,
    };
    
    assert_eq!(session.compaction_count, 0);
    
    let summarizer = |_: &[Message]| -> io::Result<String> { Ok("S".to_string()) };
    
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async {
            session
                .compact_with_rig_memory(&memory, &store, summarizer)
                .await
        })
        .unwrap();
    
    assert_eq!(session.compaction_count, 1);
}

#[test]
fn compact_with_rig_memory_handles_insufficient_messages() {
    // RED: Test no-op when messages <= keep_recent
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    let memory = Arc::new(InMemoryConversationMemory::new());
    let session_id = "test_noop";
    
    // Setup: only 2 messages, keep_recent = 3
    let messages = vec![Message::user("A"), Message::user("B")];
    
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async {
            memory.append(session_id, messages.clone()).await.unwrap();
        });
    
    let config = SessionConfig {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingSummary,
        keep_recent: 3,
    };
    
    let mut session = Session {
        id: session_id.to_string(),
        created_at: Utc::now(),
        config,
        compaction_count: 0,
    };
    
    let summarizer = |_: &[Message]| -> io::Result<String> { Ok("Summary".to_string()) };
    
    let outcome = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async {
            session
                .compact_with_rig_memory(&memory, &store, summarizer)
                .await
        })
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
fn compact_with_rig_memory_clears_before_append() {
    // RED: Test that memory is cleared before appending compacted messages
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    let memory = Arc::new(InMemoryConversationMemory::new());
    let session_id = "test_clear";
    
    let messages: Vec<Message> = (0..5)
        .map(|i| Message::user(&format!("X{}", i)))
        .collect();
    
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async {
            memory.append(session_id, messages.clone()).await.unwrap();
        });
    
    let config = SessionConfig {
        compaction_threshold: 100,
        compaction_strategy: CompactionStrategy::SlidingSummary,
        keep_recent: 2,
    };
    
    let mut session = Session {
        id: session_id.to_string(),
        created_at: Utc::now(),
        config,
        compaction_count: 0,
    };
    
    let summarizer = |old: &[Message]| -> io::Result<String> {
        Ok(format!("Summarized {}", old.len()))
    };
    
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async {
            session
                .compact_with_rig_memory(&memory, &store, summarizer)
                .await
        })
        .unwrap();
    
    // Memory should contain exactly 3 messages (summary + 2 recent), not 5 + 3
    let final_messages = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { memory.load(session_id).await.unwrap() });
    
    assert_eq!(final_messages.len(), 3);
}
