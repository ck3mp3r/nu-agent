//! GREEN tests for CompactionExecutor API surface.
//!
//! These tests verify the CompactionExecutor struct can be constructed and exposes
//! the expected API. They replace the Phase B RED stubs.

use super::*;
use crate::config::Config;
use crate::session::{JournalConversationMemory, SessionStore};

/// Helper to build a minimal Config for testing.
fn test_config() -> Config {
    Config {
        provider: "copilot".to_string(),
        provider_impl: None,
        model: "gpt-4o".to_string(),
        api_key: None,
        base_url: None,
        preamble: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tokens: None,
        max_tool_turns: None,
        temperature: None,
        read_timeout_secs: None,
        max_tool_result_bytes: None,
        model_context_tokens: None,
        context_warning_threshold: None,
        max_retries: None,
        retry_base_delay_ms: None,
        max_tool_calls_per_subturn: None,
    }
}

#[test]
fn compaction_executor_new_constructs_without_panic() {
    let config = test_config();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let temp_dir = tempfile::tempdir().unwrap();
    let memory = JournalConversationMemory::new(temp_dir.path().to_path_buf());
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    let _executor = CompactionExecutor::new(&config, &rt, &memory, &store, "test-session");
    // Construction succeeded — no panic.
}

#[test]
fn compaction_executor_session_id_accessor() {
    let config = test_config();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let temp_dir = tempfile::tempdir().unwrap();
    let memory = JournalConversationMemory::new(temp_dir.path().to_path_buf());
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    let executor = CompactionExecutor::new(&config, &rt, &memory, &store, "my-session-id");

    assert_eq!(executor.session_id(), "my-session-id");
}

#[test]
fn compaction_executor_empty_session_id() {
    let config = test_config();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let temp_dir = tempfile::tempdir().unwrap();
    let memory = JournalConversationMemory::new(temp_dir.path().to_path_buf());
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    let executor = CompactionExecutor::new(&config, &rt, &memory, &store, "session-no-tokens");

    assert_eq!(executor.session_id(), "session-no-tokens");
}
