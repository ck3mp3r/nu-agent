use std::sync::{Arc, Mutex};

use crate::types::{CallId, ToolCall, ToolFunction, ToolName};
use serde_json::json;

use super::*;
use crate::tools::authz::{
    AskChoice, AskContext, PermissionDecision, PermissionsConfig, SessionGrantCache,
};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

struct AlwaysDenyHook;

impl crate::tools::authz::AskApprovalHook for AlwaysDenyHook {
    async fn choose(
        &mut self,
        _decision: &PermissionDecision,
        _tool_name: &str,
        _source: &str,
        _args: &serde_json::Value,
        _ask_context: &AskContext,
    ) -> AskChoice {
        AskChoice::Deny
    }
}

fn make_tool_call(name: &str) -> Result<ToolCall> {
    let tool_name = ToolName::new(name).map_err(|_| "tool name must not be empty")?;
    Ok(ToolCall::new(
        CallId::from_wire("test-id"),
        ToolFunction::new(tool_name, json!({})),
    ))
}

#[tokio::test]
async fn config_tools_follow_permission_flow_table() -> Result<()> {
    // -- Setup & Fixtures
    // With safe_defaults(true), `read`/`glob`/`grep` are configured as Allow so
    // they pass without prompting; every other tool falls through to the global
    // Ask action, so the AlwaysDenyHook denies it.
    let cases: Vec<(&str, bool)> = vec![
        ("read", true),
        ("glob", true),
        ("grep", true),
        ("nu", false),
        ("edit", false),
        ("skill", false),
        ("spawn_agent", false),
        ("send_message", false),
    ];

    // -- Exec & Check
    for (tool_name, expected_pass) in cases {
        let permissions = PermissionsConfig::safe_defaults(true);
        let grant_cache = Arc::new(Mutex::new(SessionGrantCache::default()));
        let flow_context = AuthorizationFlowContext {
            ask_context: AskContext::default(),
        };
        let mut ask_hook = AlwaysDenyHook;
        let tool_call = make_tool_call(tool_name)?;

        let result = enforce_authorization_for_tool_call(
            &tool_call,
            ToolSource::Builtin,
            &permissions,
            Arc::clone(&grant_cache),
            &flow_context,
            &mut ask_hook,
        )
        .await;

        assert_eq!(
            result.is_none(),
            expected_pass,
            "tool '{tool_name}' must {}",
            if expected_pass {
                "pass the permission flow"
            } else {
                "be denied by the hook"
            }
        );
    }
    Ok(())
}

#[tokio::test]
async fn non_builtin_tools_go_through_permission_flow() -> Result<()> {
    let permissions = PermissionsConfig::safe_defaults(true);
    let grant_cache = Arc::new(Mutex::new(SessionGrantCache::default()));
    let flow_context = AuthorizationFlowContext {
        ask_context: AskContext::default(),
    };
    let mut ask_hook = AlwaysDenyHook;

    // An unknown MCP tool with deny hook should be denied
    let tool_call = make_tool_call("some_mcp_tool")?;
    let result = enforce_authorization_for_tool_call(
        &tool_call,
        ToolSource::Mcp,
        &permissions,
        Arc::clone(&grant_cache),
        &flow_context,
        &mut ask_hook,
    )
    .await;
    assert!(
        result.is_some(),
        "non-builtin tool should go through permission flow and be denied",
    );
    Ok(())
}
