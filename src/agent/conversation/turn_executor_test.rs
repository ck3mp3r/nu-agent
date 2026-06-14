//! GREEN tests for TurnExecutor API surface.
//!
//! These tests verify the TurnExecutor struct can be constructed and exposes
//! the expected API. They replace the Phase 3a RED stubs.

use super::*;
use crate::agent::tools::authz::{AskRuntimeConfig, AsyncAskHook, SessionGrantCache};
use crate::agent::tools::handler::McpToolRegistry;
use crate::config::Config;
use crate::session::JsonlConversationStore;
use crate::tools::closure::ClosureRegistry;
use crate::types::InMemoryConversationMemory;

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
    }
}

#[test]
fn turn_executor_new_constructs_without_panic() {
    let config = test_config();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut memory = InMemoryConversationMemory::new();
    let temp_dir = tempfile::tempdir().unwrap();
    let conversation_store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    let permissions = crate::agent::tools::authz::PermissionsConfig::safe_defaults(true);
    let mut session_grants = SessionGrantCache::default();
    let mut ask_hook = AsyncAskHook::new(AskRuntimeConfig::default());
    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names(Vec::<String>::new());
    let mcp_tool_server_handle = rig::tool::server::ToolServer::new().run();
    let mut last_total_tokens: Option<u64> = None;
    let final_session_id = Some("test-session".to_string());

    let _executor = TurnExecutor::new(
        &config,
        &rt,
        PermissionCtx {
            permissions: &permissions,
            session_grants: &mut session_grants,
            ask_hook: &mut ask_hook,
        },
        ConversationState {
            memory: &mut memory,
            conversation_store: &conversation_store,
            last_total_tokens: &mut last_total_tokens,
            final_session_id: &final_session_id,
        },
        ToolInfra {
            closure_registry: &closure_registry,
            mcp_registry: &mcp_registry,
            mcp_tool_server_handle: &mcp_tool_server_handle,
        },
    );
    // Construction succeeded — no panic.
}

#[test]
fn turn_executor_exposes_final_session_id() {
    let config = test_config();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut memory = InMemoryConversationMemory::new();
    let temp_dir = tempfile::tempdir().unwrap();
    let conversation_store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    let permissions = crate::agent::tools::authz::PermissionsConfig::safe_defaults(true);
    let mut session_grants = SessionGrantCache::default();
    let mut ask_hook = AsyncAskHook::new(AskRuntimeConfig::default());
    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names(Vec::<String>::new());
    let mcp_tool_server_handle = rig::tool::server::ToolServer::new().run();
    let mut last_total_tokens: Option<u64> = None;
    let final_session_id = Some("my-session-id".to_string());

    let executor = TurnExecutor::new(
        &config,
        &rt,
        PermissionCtx {
            permissions: &permissions,
            session_grants: &mut session_grants,
            ask_hook: &mut ask_hook,
        },
        ConversationState {
            memory: &mut memory,
            conversation_store: &conversation_store,
            last_total_tokens: &mut last_total_tokens,
            final_session_id: &final_session_id,
        },
        ToolInfra {
            closure_registry: &closure_registry,
            mcp_registry: &mcp_registry,
            mcp_tool_server_handle: &mcp_tool_server_handle,
        },
    );

    assert_eq!(
        executor.conversation_state.final_session_id.as_deref(),
        Some("my-session-id")
    );
}

#[test]
fn turn_executor_take_response_data_returns_none_before_execute() {
    let config = test_config();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut memory = InMemoryConversationMemory::new();
    let temp_dir = tempfile::tempdir().unwrap();
    let conversation_store = JsonlConversationStore::new(temp_dir.path().to_path_buf());
    let permissions = crate::agent::tools::authz::PermissionsConfig::safe_defaults(true);
    let mut session_grants = SessionGrantCache::default();
    let mut ask_hook = AsyncAskHook::new(AskRuntimeConfig::default());
    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names(Vec::<String>::new());
    let mcp_tool_server_handle = rig::tool::server::ToolServer::new().run();
    let mut last_total_tokens: Option<u64> = None;
    let final_session_id = None;

    let mut executor = TurnExecutor::new(
        &config,
        &rt,
        PermissionCtx {
            permissions: &permissions,
            session_grants: &mut session_grants,
            ask_hook: &mut ask_hook,
        },
        ConversationState {
            memory: &mut memory,
            conversation_store: &conversation_store,
            last_total_tokens: &mut last_total_tokens,
            final_session_id: &final_session_id,
        },
        ToolInfra {
            closure_registry: &closure_registry,
            mcp_registry: &mcp_registry,
            mcp_tool_server_handle: &mcp_tool_server_handle,
        },
    );

    assert!(executor.take_response_data().is_none());
}
