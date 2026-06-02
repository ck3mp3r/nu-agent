use rig::completion::message::ToolCall;

use crate::agent::tools::authz::{
    AskApprovalHook, AskContext, PermissionAction, PermissionEventSink, PermissionsConfig,
    SessionGrantCache, apply_ask_choice, apply_session_grant_override,
};

use super::types::{AuthorizationDeniedDetails, AuthorizationDiagnostic, ToolSource};

#[derive(Debug, Clone)]
pub(crate) struct AuthorizationFlowContext {
    pub(crate) ask_context: AskContext,
}

pub(crate) fn enforce_authorization_for_tool_call(
    tool_call: &ToolCall,
    source: ToolSource,
    permissions: &PermissionsConfig,
    grant_cache: &mut SessionGrantCache,
    flow_context: &AuthorizationFlowContext,
    ask_hook: &mut impl AskApprovalHook,
    event_sink: &mut impl PermissionEventSink,
) -> Option<AuthorizationDeniedDetails> {
    // `Builtin` tools (read-only + agent-coordination) bypass permissions entirely.
    // `BuiltinFs` tools (edit, patch) are NOT in this set — they mutate the filesystem
    // and must go through the full permission flow below, same as MCP/closure tools.
    if source == ToolSource::Builtin {
        return None;
    }

    let mut auth_decision =
        permissions.evaluate(&tool_call.function.name, &tool_call.function.arguments);
    auth_decision = apply_session_grant_override(
        auth_decision,
        grant_cache,
        &tool_call.function.name,
        source.as_str(),
        &tool_call.function.arguments,
    );
    if auth_decision.action == PermissionAction::Ask {
        let choice = ask_hook.choose_with_sink(
            &auth_decision,
            &tool_call.function.name,
            &tool_call.function.arguments,
            source.as_str(),
            &flow_context.ask_context,
            event_sink,
        );
        auth_decision = apply_ask_choice(
            auth_decision,
            choice,
            grant_cache,
            &tool_call.function.name,
            source.as_str(),
            &tool_call.function.arguments,
        );
    }
    if auth_decision.action == PermissionAction::Deny {
        let denied_details = AuthorizationDeniedDetails {
            rule_identity: auth_decision.matched_rule.identity.clone(),
            scope: auth_decision.matched_rule.scope.to_string(),
            target_field: auth_decision
                .matched_rule
                .target_field
                .map(|field| field.to_string()),
            pattern: auth_decision.matched_rule.pattern.clone(),
            diagnostics: auth_decision
                .diagnostics
                .iter()
                .map(|diagnostic| AuthorizationDiagnostic {
                    code: diagnostic.code.to_string(),
                    message: diagnostic.message.clone(),
                })
                .collect(),
        };
        return Some(denied_details);
    }
    None
}

#[cfg(test)]
#[path = "authz_gate_test.rs"]
mod authz_gate_test;
