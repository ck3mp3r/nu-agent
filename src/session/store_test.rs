use super::store::{ConversationStore, JsonlConversationStore};
use super::SessionMetadata;
use chrono::Utc;
use rig::completion::Message;
use tempfile::TempDir;

/// Mock implementation for testing the trait interface
struct MockStore {
    _temp_dir: TempDir,
}

impl MockStore {
    fn new() -> Self {
        Self {
            _temp_dir: TempDir::new().unwrap(),
        }
    }
}

impl ConversationStore for MockStore {
    fn load(&self, _session_id: &str) -> Result<Vec<Message>, Box<dyn std::error::Error>> {
        Ok(vec![])
    }

    fn append(
        &self,
        _session_id: &str,
        _messages: &[Message],
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn rewrite(
        &self,
        _session_id: &str,
        _metadata: &SessionMetadata,
        _messages: &[Message],
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn clear(&self, _session_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}

#[test]
fn trait_can_be_implemented() {
    // This test verifies that the trait compiles and can be implemented
    let store = MockStore::new();
    
    // Test load
    let messages = store.load("test-session").unwrap();
    assert_eq!(messages.len(), 0);
    
    // Test append
    let msg = Message::user("test");
    store.append("test-session", &[msg]).unwrap();
    
    // Test rewrite
    let metadata = SessionMetadata {
        metadata_type: "session".to_string(),
        session_id: "test-session".to_string(),
        created_at: Utc::now(),
        compaction_count: 0,
    };
    store.rewrite("test-session", &metadata, &[]).unwrap();
    
    // Test clear
    store.clear("test-session").unwrap();
}

#[test]
fn trait_works_with_generic_bounds() {
    // This test verifies static dispatch works
    fn generic_store_fn<T: ConversationStore>(store: &T, session_id: &str) {
        let _ = store.load(session_id);
    }
    
    let store = MockStore::new();
    generic_store_fn(&store, "test-session");
}

// JsonlConversationStore tests

#[test]
fn jsonl_store_round_trip() {
    // RED: Write test for basic round-trip functionality
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    
    let messages = vec![
        Message::user("Hello"),
        Message::assistant("Hi there!"),
        Message::user("How are you?"),
    ];
    
    let metadata = SessionMetadata {
        metadata_type: "session".to_string(),
        session_id: "test-session".to_string(),
        created_at: Utc::now(),
        compaction_count: 0,
    };
    
    // Write messages
    store.rewrite("test-session", &metadata, &messages).unwrap();
    
    // Read them back
    let loaded = store.load("test-session").unwrap();
    
    // Verify they match
    assert_eq!(loaded.len(), messages.len());
    assert_eq!(loaded, messages);
}

#[test]
fn jsonl_store_append_messages() {
    // RED: Test appending messages to existing session
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    
    let initial_messages = vec![
        Message::user("First message"),
        Message::assistant("First response"),
    ];
    
    let metadata = SessionMetadata {
        metadata_type: "session".to_string(),
        session_id: "test-session".to_string(),
        created_at: Utc::now(),
        compaction_count: 0,
    };
    
    // Write initial messages
    store.rewrite("test-session", &metadata, &initial_messages).unwrap();
    
    // Append more messages
    let additional_messages = vec![
        Message::user("Second message"),
        Message::assistant("Second response"),
    ];
    store.append("test-session", &additional_messages).unwrap();
    
    // Load and verify all messages are present
    let loaded = store.load("test-session").unwrap();
    assert_eq!(loaded.len(), 4);
    assert_eq!(loaded[0], initial_messages[0]);
    assert_eq!(loaded[1], initial_messages[1]);
    assert_eq!(loaded[2], additional_messages[0]);
    assert_eq!(loaded[3], additional_messages[1]);
}

#[test]
fn jsonl_store_rewrite_replaces_content() {
    // RED: Test that rewrite replaces all content
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    
    let initial_messages = vec![
        Message::user("Message 1"),
        Message::assistant("Response 1"),
        Message::user("Message 2"),
        Message::assistant("Response 2"),
    ];
    
    let metadata = SessionMetadata {
        metadata_type: "session".to_string(),
        session_id: "test-session".to_string(),
        created_at: Utc::now(),
        compaction_count: 0,
    };
    
    // Write initial messages
    store.rewrite("test-session", &metadata, &initial_messages).unwrap();
    
    // Rewrite with fewer messages
    let new_messages = vec![
        Message::user("New message"),
    ];
    store.rewrite("test-session", &metadata, &new_messages).unwrap();
    
    // Verify only new messages are present
    let loaded = store.load("test-session").unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0], new_messages[0]);
}

#[test]
fn jsonl_store_load_empty_returns_empty_vec() {
    // RED: Test loading non-existent session
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    
    // Load non-existent session
    let loaded = store.load("non-existent-session").unwrap();
    
    // Should return empty vec, not error
    assert_eq!(loaded.len(), 0);
}

#[test]
fn jsonl_store_clear_removes_session() {
    // RED: Test clearing a session
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    
    let messages = vec![
        Message::user("Hello"),
        Message::assistant("Hi"),
    ];
    
    let metadata = SessionMetadata {
        metadata_type: "session".to_string(),
        session_id: "test-session".to_string(),
        created_at: Utc::now(),
        compaction_count: 0,
    };
    
    // Write messages
    store.rewrite("test-session", &metadata, &messages).unwrap();
    
    // Clear the session
    store.clear("test-session").unwrap();
    
    // Load should return empty
    let loaded = store.load("test-session").unwrap();
    assert_eq!(loaded.len(), 0);
}

#[test]
fn jsonl_store_handles_corrupt_lines() {
    // RED: Test that corrupt lines are skipped with warning
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    
    let metadata = SessionMetadata {
        metadata_type: "session".to_string(),
        session_id: "test-session".to_string(),
        created_at: Utc::now(),
        compaction_count: 0,
    };
    
    // Write valid messages first
    let messages = vec![
        Message::user("Valid message 1"),
        Message::assistant("Valid response 1"),
    ];
    store.rewrite("test-session", &metadata, &messages).unwrap();
    
    // Manually append a corrupt line to the file
    let session_path = temp_dir.path().join("test-session.jsonl");
    std::fs::write(
        &session_path,
        std::fs::read_to_string(&session_path).unwrap() + "\nthis is not valid json\n"
            + &serde_json::to_string(&Message::user("Valid message 2")).unwrap()
            + "\n",
    ).unwrap();
    
    // Load should skip corrupt line but return valid messages
    let loaded = store.load("test-session").unwrap();
    assert_eq!(loaded.len(), 3); // 2 original + 1 after corrupt line
    assert_eq!(loaded[0], messages[0]);
    assert_eq!(loaded[1], messages[1]);
    assert_eq!(loaded[2], Message::user("Valid message 2"));
}

#[test]
fn jsonl_store_append_creates_session_if_missing() {
    // RED: Test that append creates session if it doesn't exist
    let temp_dir = TempDir::new().unwrap();
    let store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    
    let messages = vec![
        Message::user("First message in new session"),
    ];
    
    // Append to non-existent session
    store.append("new-session", &messages).unwrap();
    
    // Load and verify
    let loaded = store.load("new-session").unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0], messages[0]);
}

