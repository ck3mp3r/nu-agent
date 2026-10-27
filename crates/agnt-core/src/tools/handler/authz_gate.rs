use std::sync::{Arc, Mutex};

use serde_json::Value as JsonValue;

use crate::types::ToolCall;

use crate::tools::authz::{
    AskApprovalHook, AskContext, PermissionAction, PermissionDecision, PermissionsConfig,
    SessionGrantCache, apply_ask_choice, apply_session_grant_override,
};

use super::types::ToolSource;

#[derive(Debug, Clone)]
pub struct AuthorizationFlowContext {
    pub ask_context: AskContext,
}

/// Final authorization outcome for one tool call.
///
/// `decision` is the decision after the session-grant override and the ask
/// hook have been applied. `deny_reason` is `Some` only when the final action
/// is [`PermissionAction::Deny`], and carries the rule-derived reason string.
#[derive(Debug, Clone)]
pub struct AuthzOutcome {
    pub decision: PermissionDecision,
    pub deny_reason: Option<String>,
}

pub async fn enforce_authorization_for_tool_call(
    tool_call: &ToolCall,
    source: ToolSource,
    permissions: &PermissionsConfig,
    grant_cache: Arc<Mutex<SessionGrantCache>>,
    flow_context: &AuthorizationFlowContext,
    ask_hook: &mut impl AskApprovalHook,
) -> AuthzOutcome {
    let mut auth_decision =
        permissions.evaluate(&tool_call.function.name, &tool_call.function.arguments);
    log::debug!(
        "Authz evaluate: tool={} source={:?} action={:?}",
        tool_call.function.name,
        source,
        auth_decision.action
    );
    let pre_grant_action = auth_decision.action;

    // Lock, check session grants, unlock — no .await while holding the lock.
    {
        let grants = grant_cache.lock().expect("grant_cache lock");
        auth_decision = apply_session_grant_override(
            auth_decision,
            &grants,
            &tool_call.function.name,
            source.as_str(),
            &tool_call.function.arguments,
        );
    }

    if pre_grant_action == PermissionAction::Ask && auth_decision.action != PermissionAction::Ask {
        log::debug!(
            "Authz: session grant override for tool={}",
            tool_call.function.name
        );
    }
    if auth_decision.action == PermissionAction::Ask {
        // The ask hook still takes the arguments as a `JsonValue`; the tool
        // call carries them as a map.
        let args_value = JsonValue::Object(tool_call.function.arguments.clone());
        let choice = ask_hook
            .choose(
                &auth_decision,
                &tool_call.function.name,
                source.as_str(),
                &args_value,
                &flow_context.ask_context,
            )
            .await;

        // Lock again to write the result — no .await while holding the lock.
        {
            let mut grants = grant_cache.lock().expect("grant_cache lock");
            auth_decision = apply_ask_choice(
                auth_decision,
                choice,
                &mut grants,
                &tool_call.function.name,
                source.as_str(),
                &tool_call.function.arguments,
            );
        }
    }
    if auth_decision.action == PermissionAction::Deny {
        log::warn!(
            "Authz DENIED: tool={} rule={} scope={} pattern={:?}",
            tool_call.function.name,
            auth_decision.matched_rule.identity,
            auth_decision.matched_rule.scope,
            auth_decision.matched_rule.pattern
        );
        let deny_reason = format!(
            "Permission denied by rule '{}' (scope: {})",
            auth_decision.matched_rule.identity, auth_decision.matched_rule.scope
        );
        return AuthzOutcome {
            decision: auth_decision,
            deny_reason: Some(deny_reason),
        };
    }
    AuthzOutcome {
        decision: auth_decision,
        deny_reason: None,
    }
}

#[cfg(test)]
#[path = "../../../test/tools/handler/authz_gate.rs"]
mod authz_gate_test;
