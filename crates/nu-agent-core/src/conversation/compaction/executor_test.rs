//! GREEN tests for CompactionExecutor API surface.
//!
//! These tests verify the CompactionExecutor struct can be constructed and exposes
//! the expected API. They replace the Phase B RED stubs.

use super::*;
use crate::compaction::CompactionParams;
use crate::config::Config;
use crate::session::{CachedMemory, FsSessionStore};
use std::sync::Arc;

/// Helper to build a minimal Config for testing.
fn test_config() -> Config {
    Config {
        a2a_port: None,
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
        additional_params: None,
        a2a_enabled: None,
        session_store_type: None,
    }
}

#[test]
fn compaction_executor_new_constructs_without_panic() {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let store = Arc::new(FsSessionStore::new(temp_dir.path().to_path_buf()));
    let memory = CachedMemory::<FsSessionStore>::new(Arc::clone(&store));

    let _executor = CompactionExecutor::new(
        &config,
        &memory,
        "test-session",
        CompactionParams::default(),
        crate::bus::create_bus(),
    );
    // Construction succeeded — no panic.
}
