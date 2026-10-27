use std::sync::{Arc, Mutex};

use crate::types::{CallId, ToolCall, ToolFunction, ToolName};
use serde_json::json;

use super::*;
use crate::tools::authz::{
    AskChoice, AskContext, PermissionAction, PermissionDecision, PermissionsConfig,
    SessionGrantCache,
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

        let outcome = enforce_authorization_for_tool_call(
            &tool_call,
            ToolSource::Builtin,
            &permissions,
            Arc::clone(&grant_cache),
            &flow_context,
            &mut ask_hook,
        )
        .await;

        assert_eq!(
            outcome.deny_reason.is_none(),
            expected_pass,
            "tool '{tool_name}' must {}",
            if expected_pass {
                "pass the permission flow"
            } else {
                "be denied by the hook"
            }
        );

        if expected_pass {
            assert_eq!(
                outcome.decision.action,
                PermissionAction::Allow,
                "tool '{tool_name}' must carry an Allow decision"
            );
            assert!(
                !outcome.decision.matched_rule.identity.is_empty(),
                "tool '{tool_name}' must carry the matched rule identity"
            );
        } else {
            assert_eq!(
                outcome.decision.action,
                PermissionAction::Deny,
                "tool '{tool_name}' must carry a Deny decision"
            );
            let expected_reason = format!(
                "Permission denied by rule '{}' (scope: {})",
                outcome.decision.matched_rule.identity, outcome.decision.matched_rule.scope
            );
            assert_eq!(
                outcome.deny_reason.as_deref(),
                Some(expected_reason.as_str()),
                "tool '{tool_name}' must carry the rule-derived deny reason"
            );
        }
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
    let outcome = enforce_authorization_for_tool_call(
        &tool_call,
        ToolSource::Mcp,
        &permissions,
        Arc::clone(&grant_cache),
        &flow_context,
        &mut ask_hook,
    )
    .await;
    assert!(
        outcome.deny_reason.is_some(),
        "non-builtin tool should go through permission flow and be denied",
    );
    assert_eq!(
        outcome.decision.action,
        PermissionAction::Deny,
        "non-builtin tool should carry a Deny decision",
    );
    assert!(
        !outcome.decision.matched_rule.identity.is_empty(),
        "non-builtin tool should carry the matched rule identity",
    );
    Ok(())
}

#[tokio::test]
async fn session_grant_override_returns_allow_with_grant_rule() -> Result<()> {
    // -- Setup & Fixtures
    // `nu` falls through to the global Ask action. Seed the grant cache with an
    // allow-always grant for that exact decision, so the gate's grant override
    // turns the Ask into an Allow without invoking the ask hook.
    let permissions = PermissionsConfig::safe_defaults(true);
    let grant_cache = Arc::new(Mutex::new(SessionGrantCache::default()));
    let flow_context = AuthorizationFlowContext {
        ask_context: AskContext::default(),
    };
    let mut ask_hook = AlwaysDenyHook;
    let tool_call = make_tool_call("nu")?;

    let ask_decision = permissions.evaluate("nu", &tool_call.function.arguments);
    assert_eq!(
        ask_decision.action,
        PermissionAction::Ask,
        "fixture requires `nu` to evaluate to Ask",
    );
    let grant_identity = ask_decision.matched_rule.identity.clone();
    grant_cache
        .lock()
        .expect("grant_cache lock")
        .insert_allow_always(
            &ask_decision,
            "nu",
            ToolSource::Builtin.as_str(),
            &tool_call.function.arguments,
        );

    // -- Exec
    let outcome = enforce_authorization_for_tool_call(
        &tool_call,
        ToolSource::Builtin,
        &permissions,
        Arc::clone(&grant_cache),
        &flow_context,
        &mut ask_hook,
    )
    .await;

    // -- Check
    assert_eq!(
        outcome.decision.action,
        PermissionAction::Allow,
        "session grant must override Ask to Allow",
    );
    assert_eq!(
        outcome.decision.matched_rule.identity, grant_identity,
        "overridden decision must carry the granting rule identity",
    );
    assert!(
        outcome.deny_reason.is_none(),
        "an allowed call must carry no deny reason",
    );
    Ok(())
}
