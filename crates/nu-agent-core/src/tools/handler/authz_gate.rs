use std::sync::{Arc, Mutex};

use crate::types::ToolCall;

use crate::tools::authz::{
    AskApprovalHook, AskContext, PermissionAction, PermissionEventSink, PermissionsConfig,
    SessionGrantCache, apply_ask_choice, apply_session_grant_override,
};

use super::types::ToolSource;

#[derive(Debug, Clone)]
pub struct AuthorizationFlowContext {
    pub ask_context: AskContext,
}

pub async fn enforce_authorization_for_tool_call(
    tool_call: &ToolCall,
    source: ToolSource,
    permissions: &PermissionsConfig,
    grant_cache: Arc<Mutex<SessionGrantCache>>,
    flow_context: &AuthorizationFlowContext,
    ask_hook: &mut impl AskApprovalHook,
    event_sink: &mut impl PermissionEventSink,
) -> bool {
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
        let choice = ask_hook
            .choose(
                &auth_decision,
                &tool_call.function.name,
                source.as_str(),
                &tool_call.function.arguments,
                &flow_context.ask_context,
                Some(event_sink),
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
        return true;
    }
    false
}

#[cfg(test)]
#[path = "authz_gate_test.rs"]
mod authz_gate_test;
