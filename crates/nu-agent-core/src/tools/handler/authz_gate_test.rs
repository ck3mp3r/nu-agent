use std::sync::{Arc, Mutex};

use crate::types::{ToolCall, ToolCallId, ToolFunction};
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
        ToolCallId::new_or_mint("test-id"),
        ToolFunction::new(name.to_string(), json!({})),
    )
}

#[tokio::test]
async fn config_allow_tools_pass_permission_flow() {
    // With safe_defaults(true), `read` is configured as Allow so it passes
    // without prompting, even though the hook would otherwise deny.
    let permissions = PermissionsConfig::safe_defaults(true);
    let grant_cache = Arc::new(Mutex::new(SessionGrantCache::default()));
    let flow_context = AuthorizationFlowContext {
        ask_context: AskContext::default(),
    };
    let mut ask_hook = AlwaysDenyHook;
    let mut sink = NoopSink;

    for tool_name in ["read", "glob", "grep"] {
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
            "tool '{}' configured as allow should pass the permission flow",
            tool_name,
        );
    }
}

#[tokio::test]
async fn config_ask_tools_prompt_through_permission_flow() {
    // With safe_defaults(true), tools not listed as allow fall through to the
    // global Ask action, so the AlwaysDenyHook denies them.
    let permissions = PermissionsConfig::safe_defaults(true);
    let grant_cache = Arc::new(Mutex::new(SessionGrantCache::default()));
    let flow_context = AuthorizationFlowContext {
        ask_context: AskContext::default(),
    };
    let mut ask_hook = AlwaysDenyHook;
    let mut sink = NoopSink;

    for tool_name in ["nu", "edit", "skill", "spawn_agent", "send_message"] {
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
            result,
            "tool '{}' not configured as allow should be denied by the hook",
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
