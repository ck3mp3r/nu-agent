use std::collections::VecDeque;

use agnt_core::protocol::event::{
    PermissionDecision, PermissionDecisionSubmission, PermissionRequestContext,
};

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

impl PermissionPrompt {
    /// Build the prompt the card renders from one request.
    pub fn from_context(request_id: &str, context: &PermissionRequestContext) -> Self {
        Self {
            request_id: request_id.to_string(),
            matched_rule_identity: context.matched_rule_identity.clone(),
            tool: context.tool.clone(),
            source: context.source.clone(),
            mode: context.mode.clone(),
            scope: context.scope.clone(),
            pattern: context.pattern.clone(),
            target_field: context.target_field.clone(),
            summary: context.summary.clone(),
        }
    }
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

    /// Close the open prompt without submitting a decision.
    ///
    /// Used when the request can no longer be answered: the turn was cancelled
    /// or has ended, so the resolver already dropped the request. Queued
    /// submissions are left untouched.
    pub fn close_prompt(&mut self) {
        self.prompt = None;
    }

    /// Close the open prompt only when it belongs to `request_id`.
    ///
    /// Returns whether the prompt was closed. A decision timeout for another
    /// request must leave the open card alone.
    pub fn close_prompt_if(&mut self, request_id: &str) -> bool {
        let matches_open = self
            .prompt
            .as_ref()
            .is_some_and(|prompt| prompt.request_id == request_id);
        if matches_open {
            self.prompt = None;
        }
        matches_open
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
}
