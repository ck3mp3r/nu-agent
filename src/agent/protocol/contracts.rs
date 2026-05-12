use nu_protocol::{LabeledError, Span, Value};

use crate::agent::protocol::{
    compaction::{CompactionTriggerDecision, CompactionTriggerSource},
    event::{PermissionDecisionSubmission, UiEvent},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum McpUsabilityState {
    Enabled,
    Disabled,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct McpToggleRequest {
    pub server_name: String,
    pub enable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SharedUiAction {
    Help,
    Status,
    Mcps,
    Models,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UiMessageSnapshot {
    role: String,
    content: String,
    tool_arguments: Option<String>,
    tool_result: Option<String>,
    tool_success: Option<bool>,
    usage: Option<UiMessageUsageSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UiMessageUsageSnapshot {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    total_tokens: Option<u64>,
}

impl UiMessageSnapshot {
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
            tool_arguments: None,
            tool_result: None,
            tool_success: None,
            usage: None,
        }
    }

    pub fn with_tool_details(
        mut self,
        arguments: Option<String>,
        result: Option<String>,
        success: Option<bool>,
    ) -> Self {
        self.tool_arguments = arguments;
        self.tool_result = result;
        self.tool_success = success;
        self
    }

    pub fn role(&self) -> &str {
        &self.role
    }

    pub fn with_usage(mut self, usage: UiMessageUsageSnapshot) -> Self {
        if usage.input_tokens.is_some()
            || usage.output_tokens.is_some()
            || usage.total_tokens.is_some()
        {
            self.usage = Some(usage);
        }
        self
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn tool_arguments(&self) -> Option<&str> {
        self.tool_arguments.as_deref()
    }

    pub fn tool_success(&self) -> Option<bool> {
        self.tool_success
    }

    pub fn usage(&self) -> Option<UiMessageUsageSnapshot> {
        self.usage
    }
}

impl UiMessageUsageSnapshot {
    pub fn new(
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
        total_tokens: Option<u64>,
    ) -> Self {
        Self {
            input_tokens,
            output_tokens,
            total_tokens,
        }
    }

    pub fn input_tokens(&self) -> Option<u64> {
        self.input_tokens
    }

    pub fn output_tokens(&self) -> Option<u64> {
        self.output_tokens
    }

    pub fn total_tokens(&self) -> Option<u64> {
        self.total_tokens
    }
}

pub(crate) trait ProgressUi {
    fn emit(&mut self, event: &UiEvent);
    fn flush(&mut self);
    fn take_cancel_requested(&self) -> bool;
    fn cancellation_value(&self, _span: Span) -> Option<Value> {
        None
    }
}

pub(crate) trait InteractiveUi: ProgressUi {
    fn pump_once(&mut self);
    fn take_submitted_prompt(&mut self) -> Option<String>;
    fn take_next_model_picker_launch_request(&mut self) -> bool {
        false
    }
    fn take_next_mcp_toggle_request(&mut self) -> Option<McpToggleRequest> {
        None
    }
    fn take_next_model_switch_request(&mut self) -> Option<String> {
        None
    }
    fn take_next_permission_decision_submission(&mut self) -> Option<PermissionDecisionSubmission> {
        None
    }
    fn set_mcp_server_state(&mut self, _server_name: &str, _state: McpUsabilityState) {}
    fn set_mcp_visible_tool_count_by_server_name(&mut self, _server_name: &str, _count: usize) {}
    fn set_mcp_visible_tool_names_by_server_name(
        &mut self,
        _server_name: &str,
        _names: Vec<String>,
    ) {
    }
    fn set_mcp_server_state_with_details(
        &mut self,
        server_name: &str,
        state: McpUsabilityState,
        _reason: Option<String>,
        _llm_visible_mcp_tool_count: usize,
    ) {
        self.set_mcp_server_state(server_name, state);
    }
    fn quit_requested(&self) -> bool;
    fn execute_shared_ui_action(&mut self, _action: SharedUiAction) -> bool {
        false
    }
    fn set_active_model_identity(&mut self, _active_model_identity: &str) {}
    fn fatal_error(&self) -> Option<&str>;
    fn hydrate_transcript_from_messages(
        &mut self,
        messages: impl IntoIterator<Item = UiMessageSnapshot>,
    );
}

pub(crate) trait ConversationRuntime {
    fn execute_turn<U: ProgressUi>(
        &mut self,
        ui: &mut U,
        prompt: String,
        context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError>;

    fn set_mcp_server_enabled(
        &mut self,
        _server_name: &str,
        _enabled: bool,
    ) -> Result<McpUsabilityState, String> {
        Ok(McpUsabilityState::Disabled)
    }

    fn llm_visible_mcp_tool_count(&self) -> usize {
        0
    }

    fn llm_visible_mcp_tool_count_for_server(&self, _server_name: &str) -> usize {
        0
    }

    fn llm_visible_mcp_tool_names_by_server(&self) -> Vec<(String, Vec<String>)> {
        Vec::new()
    }

    fn switch_model(&mut self, _model_spec: &str) -> Result<String, String> {
        Err("model switching not supported".to_string())
    }

    fn active_model_identity(&self) -> String {
        "unknown/unknown".to_string()
    }

    fn evaluate_auto_compaction(&mut self) -> Option<CompactionTriggerDecision> {
        None
    }

    fn execute_compaction_trigger<U: ProgressUi>(
        &mut self,
        _ui: &mut U,
        _source: CompactionTriggerSource,
    ) -> Result<(), String> {
        Ok(())
    }
}
