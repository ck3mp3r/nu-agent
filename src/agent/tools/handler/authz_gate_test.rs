use rig::completion::message::{ToolCall, ToolFunction};
use serde_json::json;

use super::*;
use crate::agent::tools::authz::{
    AskChoice, AskContext, PermissionDecision, PermissionEventSink, PermissionsConfig,
    SessionGrantCache,
};

struct AlwaysDenyHook;

impl crate::agent::tools::authz::AskApprovalHook for AlwaysDenyHook {
    fn choose(
        &mut self,
        _decision: &PermissionDecision,
        _tool_name: &str,
        _args: &serde_json::Value,
    ) -> AskChoice {
        AskChoice::Deny
    }
}

struct NoopSink;

impl PermissionEventSink for NoopSink {
    fn emit(&mut self, _event: crate::agent::protocol::event::UiEvent) {}
}

fn make_tool_call(name: &str) -> ToolCall {
    ToolCall::new(
        "test-id".to_string(),
        ToolFunction::new(name.to_string(), json!({})),
    )
}

#[test]
fn builtin_tools_bypass_permission_flow() {
    // Even with deny-all permissions, builtin tools should be allowed
    let permissions = PermissionsConfig::safe_defaults();
    let mut grant_cache = SessionGrantCache::default();
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
            &mut grant_cache,
            &flow_context,
            &mut ask_hook,
            &mut sink,
        );
        assert!(
            result.is_none(),
            "builtin tool '{}' should be auto-allowed but was denied",
            tool_name,
        );
    }
}

#[test]
fn builtin_fs_tools_go_through_permission_flow() {
    let permissions = PermissionsConfig::safe_defaults();
    let mut grant_cache = SessionGrantCache::default();
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
            &mut grant_cache,
            &flow_context,
            &mut ask_hook,
            &mut sink,
        );
        assert!(
            result.is_some(),
            "BuiltinFs tool '{}' should go through permission flow and be denied by AlwaysDenyHook",
            tool_name,
        );
    }
}

#[test]
fn non_builtin_tools_go_through_permission_flow() {
    let permissions = PermissionsConfig::safe_defaults();
    let mut grant_cache = SessionGrantCache::default();
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
        &mut grant_cache,
        &flow_context,
        &mut ask_hook,
        &mut sink,
    );
    assert!(
        result.is_some(),
        "non-builtin tool should go through permission flow and be denied",
    );
}
