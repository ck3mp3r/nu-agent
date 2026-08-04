use std::sync::{Arc, Mutex};

use crate::types::{ToolCall, ToolFunction};
use serde_json::json;

use super::*;
use crate::tools::authz::{
    AskChoice, AskContext, PermissionDecision, PermissionEventSink, PermissionsConfig,
    SessionGrantCache,
};

use async_trait::async_trait;

struct AlwaysDenyHook;

#[async_trait]
impl crate::tools::authz::AskApprovalHook for AlwaysDenyHook {
    async fn choose<S: crate::tools::authz::PermissionEventSink + Send>(
        &mut self,
        _decision: &PermissionDecision,
        _tool_name: &str,
        _source: &str,
        _args: &serde_json::Value,
        _ask_context: &AskContext,
        _sink: Option<&mut S>,
    ) -> AskChoice {
        AskChoice::Deny
    }
}

struct NoopSink;

impl PermissionEventSink for NoopSink {
    fn emit(&mut self, _event: crate::protocol::event::UiEvent) {}
}

fn make_tool_call(name: &str) -> ToolCall {
    ToolCall::new(
        "test-id".to_string(),
        ToolFunction::new(name.to_string(), json!({})),
    )
}

#[tokio::test]
async fn builtin_tools_bypass_permission_flow() {
    // Even with deny-all permissions, builtin tools should be allowed
    let permissions = PermissionsConfig::safe_defaults(true);
    let grant_cache = Arc::new(Mutex::new(SessionGrantCache::default()));
    let flow_context = AuthorizationFlowContext {
        ask_context: AskContext::default(),
    };
    let mut ask_hook = AlwaysDenyHook;
    let mut sink = NoopSink;

    for tool_name in [
        "read",
        "skill",
        "send_message",
        "list_agents",
        "spawn_agent",
    ] {
        let tool_call = make_tool_call(tool_name);
        let result = enforce_authorization_for_tool_call(
            &tool_call,
            ToolSource::Builtin,
            &permissions,
            Arc::clone(&grant_cache),
            &flow_context,
            &mut ask_hook,
            &mut sink,
        )
        .await;
        assert!(
            !result,
            "builtin tool '{}' should be auto-allowed but was denied",
            tool_name,
        );
    }
}

#[tokio::test]
async fn fs_tools_go_through_permission_flow() {
    let permissions = PermissionsConfig::safe_defaults(true);
    let grant_cache = Arc::new(Mutex::new(SessionGrantCache::default()));
    let flow_context = AuthorizationFlowContext {
        ask_context: AskContext::default(),
    };
    let mut ask_hook = AlwaysDenyHook;
    let mut sink = NoopSink;

    for tool_name in ["edit", "patch"] {
        let tool_call = make_tool_call(tool_name);
        let result = enforce_authorization_for_tool_call(
            &tool_call,
            ToolSource::BuiltinFs,
            &permissions,
            Arc::clone(&grant_cache),
            &flow_context,
            &mut ask_hook,
            &mut sink,
        )
        .await;
        assert!(
            result,
            "BuiltinFs tool '{}' should go through permission flow and be denied by AlwaysDenyHook",
            tool_name,
        );
    }
}

#[tokio::test]
async fn non_builtin_tools_go_through_permission_flow() {
    let permissions = PermissionsConfig::safe_defaults(true);
    let grant_cache = Arc::new(Mutex::new(SessionGrantCache::default()));
    let flow_context = AuthorizationFlowContext {
        ask_context: AskContext::default(),
    };
    let mut ask_hook = AlwaysDenyHook;
    let mut sink = NoopSink;

    // An unknown MCP tool with deny hook should be denied
    let tool_call = make_tool_call("some_mcp_tool");
    let result = enforce_authorization_for_tool_call(
        &tool_call,
        ToolSource::Mcp,
        &permissions,
        Arc::clone(&grant_cache),
        &flow_context,
        &mut ask_hook,
        &mut sink,
    )
    .await;
    assert!(
        result,
        "non-builtin tool should go through permission flow and be denied",
    );
}
