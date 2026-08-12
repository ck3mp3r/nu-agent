use super::*;

impl AppState {
    pub fn open_permission_prompt(&mut self, prompt: PermissionPrompt) {
        self.permission_prompt = Some(prompt);
        self.status_line = "Permission required".to_string();
        self.scroll_transcript_to_bottom();
        self.ensure_invariants();
    }

    pub fn focus_transcript_pane(&mut self) {
        self.pane_focus = PaneFocus::Transcript;
    }

    pub fn latest_in_progress_tool_key_for_tool(&self, tool_name: &str) -> Option<String> {
        let base_tool_name = tool_name.split('(').next().unwrap_or(tool_name);

        self.tool_call_items
            .iter()
            .rev()
            .find(|item| {
                item.status == ToolCallStatus::InProgress
                    && item
                        .key
                        .split_once('\n')
                        .map(|(name, _)| name == base_tool_name)
                        .unwrap_or(false)
            })
            .map(|item| item.key.clone())
    }

    pub fn has_permission_prompt(&self) -> bool {
        self.permission_prompt.is_some()
    }

    pub fn submit_permission_decision(&mut self, decision: PermissionDecision) -> bool {
        let Some(prompt) = self.permission_prompt.as_ref() else {
            return false;
        };
        self.pending_permission_decisions
            .push_back(PermissionDecisionSubmission {
                request_id: prompt.request_id.clone(),
                decision,
                matched_rule_identity: prompt.matched_rule_identity.clone(),
            });
        self.permission_prompt = None;
        self.ensure_invariants();
        true
    }

    pub fn take_next_permission_decision_submission(
        &mut self,
    ) -> Option<PermissionDecisionSubmission> {
        self.pending_permission_decisions.pop_front()
    }
}
