//! GREEN tests for TurnExecutor API surface.
//!
//! These tests verify the TurnExecutor struct can be constructed and exposes
//! the expected API. They replace the Phase 3a RED stubs.

use super::*;
use crate::config::Config;
use crate::tools::authz::{AskRuntimeConfig, AsyncAskHook, SessionGrantCache};
use crate::tools::closure::ClosureRegistry;
use crate::tools::handler::McpToolRegistry;

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
    let temp_dir = tempfile::tempdir().unwrap();
    let mut memory_state =
        super::super::super::state::memory::MemoryState::new(temp_dir.path().to_path_buf());
    let mut permission_state = super::super::super::state::permission::PermissionState::new(
        crate::tools::authz::PermissionsConfig::safe_defaults(true),
        SessionGrantCache::default(),
        AsyncAskHook::new(AskRuntimeConfig::default()),
        String::new(),
    );
    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names(Vec::<String>::new());
    let mcp_tool_server_handle = rig::tool::server::ToolServer::new().run();

    let _executor = TurnExecutor::new(
        &config,
        &rt,
        &mut permission_state,
        &mut memory_state,
        ToolInfra {
            closure_registry: &closure_registry,
            mcp_registry: &mcp_registry,
            tool_server_handle: mcp_tool_server_handle,
            visible_tool_definitions: vec![],
        },
    );
    // Construction succeeded — no panic.
}

#[test]
fn turn_executor_exposes_memory_state() {
    let config = test_config();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let temp_dir = tempfile::tempdir().unwrap();
    let mut memory_state =
        super::super::super::state::memory::MemoryState::new(temp_dir.path().to_path_buf());
    let mut permission_state = super::super::super::state::permission::PermissionState::new(
        crate::tools::authz::PermissionsConfig::safe_defaults(true),
        SessionGrantCache::default(),
        AsyncAskHook::new(AskRuntimeConfig::default()),
        String::new(),
    );
    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names(Vec::<String>::new());
    let mcp_tool_server_handle = rig::tool::server::ToolServer::new().run();

    let executor = TurnExecutor::new(
        &config,
        &rt,
        &mut permission_state,
        &mut memory_state,
        ToolInfra {
            closure_registry: &closure_registry,
            mcp_registry: &mcp_registry,
            tool_server_handle: mcp_tool_server_handle,
            visible_tool_definitions: vec![],
        },
    );

    // Verify memory_state is accessible and last_total_tokens starts None
    assert!(executor.memory_state.last_total_tokens().is_none());
}

#[test]
fn turn_executor_take_response_data_returns_none_before_execute() {
    let config = test_config();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let temp_dir = tempfile::tempdir().unwrap();
    let mut memory_state =
        super::super::super::state::memory::MemoryState::new(temp_dir.path().to_path_buf());
    let mut permission_state = super::super::super::state::permission::PermissionState::new(
        crate::tools::authz::PermissionsConfig::safe_defaults(true),
        SessionGrantCache::default(),
        AsyncAskHook::new(AskRuntimeConfig::default()),
        String::new(),
    );
    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names(Vec::<String>::new());
    let mcp_tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &rt,
        &mut permission_state,
        &mut memory_state,
        ToolInfra {
            closure_registry: &closure_registry,
            mcp_registry: &mcp_registry,
            tool_server_handle: mcp_tool_server_handle,
            visible_tool_definitions: vec![],
        },
    );

    assert!(executor.take_response_data().is_none());
}
