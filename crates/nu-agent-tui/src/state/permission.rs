use std::collections::VecDeque;

use nu_agent_core::bus::PermissionEvent;
use nu_agent_core::protocol::event::{PermissionDecision, PermissionDecisionSubmission};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionPrompt {
    pub request_id: String,
    pub matched_rule_identity: String,
    pub tool: String,
    pub source: String,
    pub mode: Option<String>,
    pub scope: String,
    pub pattern: String,
    pub target_field: Option<String>,
    pub summary: String,
}

#[derive(Debug, Clone, Default)]
pub struct PermissionState {
    prompt: Option<PermissionPrompt>,
    pending_decisions: VecDeque<PermissionDecisionSubmission>,
}

impl PermissionState {
    pub fn open_prompt(&mut self, prompt: PermissionPrompt) {
        self.prompt = Some(prompt);
    }

    pub fn has_prompt(&self) -> bool {
        self.prompt.is_some()
    }

    pub fn submit_decision(&mut self, decision: PermissionDecision) -> bool {
        let Some(prompt) = self.prompt.as_ref() else {
            return false;
        };
        self.pending_decisions
            .push_back(PermissionDecisionSubmission {
                request_id: prompt.request_id.clone(),
                decision,
                matched_rule_identity: prompt.matched_rule_identity.clone(),
            });
        self.prompt = None;
        true
    }

    pub fn take_next_submission(&mut self) -> Option<PermissionDecisionSubmission> {
        self.pending_decisions.pop_front()
    }

    pub fn reduce_permission_event(&mut self, event: PermissionEvent) -> bool {
        match event {
            PermissionEvent::Requested {
                request_id,
                context,
            } => {
                self.open_prompt(PermissionPrompt {
                    request_id,
                    matched_rule_identity: context.matched_rule_identity.clone(),
                    tool: context.tool.clone(),
                    source: context.source.clone(),
                    mode: context.mode.clone(),
                    scope: context.scope.clone(),
                    pattern: context.pattern.clone(),
                    target_field: context.target_field.clone(),
                    summary: context.summary.clone(),
                });
                true
            }
            PermissionEvent::DecisionSubmitted { .. }
            | PermissionEvent::DecisionTimedOut { .. }
            | PermissionEvent::DecisionIgnored { .. } => false,
        }
    }
}
