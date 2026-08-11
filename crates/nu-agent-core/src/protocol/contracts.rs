use nu_protocol::{LabeledError, Span, Value};

use crate::protocol::event::{PermissionDecisionSubmission, ToolDisplay, UiEvent};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpUsabilityState {
    Enabled,
    Disabled,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpToggleRequest {
    pub server_name: String,
    pub enable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SharedUiAction {
    Help,
    Status,
    Mcps,
    Models,
    Agents,
    Sessions,
    Themes,
    Skills,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiMessageSnapshot {
    role: String,
    content: String,
    tool_name: Option<String>,
    tool_arguments: Option<String>,
    tool_result: Option<String>,
    tool_success: Option<bool>,
    tool_display: Option<ToolDisplay>,
    pub usage: Option<UiMessageUsageSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UiMessageUsageSnapshot {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

impl UiMessageSnapshot {
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
            tool_name: None,
            tool_arguments: None,
            tool_result: None,
            tool_success: None,
            tool_display: None,
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

    pub fn with_tool_name(mut self, name: String) -> Self {
        self.tool_name = Some(name);
        self
    }

    pub fn with_tool_display(mut self, display: ToolDisplay) -> Self {
        self.tool_display = Some(display);
        self
    }

    pub fn role(&self) -> &str {
        &self.role
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn tool_name(&self) -> Option<&str> {
        self.tool_name.as_deref()
    }

    pub fn tool_arguments(&self) -> Option<&str> {
        self.tool_arguments.as_deref()
    }

    pub fn tool_success(&self) -> Option<bool> {
        self.tool_success
    }

    pub fn take_tool_display(&mut self) -> Option<ToolDisplay> {
        self.tool_display.take()
    }

    pub fn tool_display(&self) -> Option<&ToolDisplay> {
        self.tool_display.as_ref()
    }

    pub fn usage(&self) -> Option<UiMessageUsageSnapshot> {
        self.usage
    }
}

impl UiMessageUsageSnapshot {
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

pub trait ProgressUi {
    fn emit(&mut self, event: &UiEvent);
    fn flush(&mut self);
    fn take_cancel_requested(&self) -> bool;
    fn emit_batch(&mut self, events: &[UiEvent]) {
        for event in events {
            self.emit(event);
        }
    }
    fn external_cancel_token(&self) -> Option<tokio_util::sync::CancellationToken> {
        None
    }
}

pub trait UserInputUi {
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
    fn take_next_agent_picker_launch_request(&mut self) -> bool {
        false
    }
    fn take_next_agent_switch_request(&mut self) -> Option<String> {
        None
    }
    fn take_next_session_picker_launch_request(&mut self) -> bool {
        false
    }
    fn take_next_theme_picker_launch_request(&mut self) -> bool {
        false
    }
    fn take_next_session_switch_request(&mut self) -> Option<String> {
        None
    }
}

pub trait DisplayStateUi {
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
    fn execute_shared_ui_action(&mut self, _action: SharedUiAction) -> bool {
        false
    }
    fn set_active_model_identity(&mut self, _active_model_identity: &str) {}
    fn set_active_agent_identity(&mut self, _name: &str) {}
    fn set_active_persona_icon(&mut self, _icon: Option<String>) {}
    fn set_context_window_max_tokens(&mut self, _max_tokens: Option<u64>) {}
    fn display_incoming_message(&mut self, _text: &str) {}
}

pub trait LifecycleUi {
    fn pump_once(&mut self);
    fn quit_requested(&self) -> bool;
    fn fatal_error(&self) -> Option<&str>;
}

pub trait TranscriptUi {
    fn hydrate_transcript_from_messages(
        &mut self,
        messages: impl IntoIterator<Item = UiMessageSnapshot>,
        last_total_tokens: Option<u64>,
    );
    fn clear_transcript(&mut self) {}
    fn push_startup_logo(&mut self) {}
}

/// Minimal runtime required for a single turn. Used by run_single_turn.
pub trait CoreRuntime {
    fn execute_turn<U: ProgressUi + Send>(
        &mut self,
        ui: &mut U,
        prompt: String,
        context: Option<String>,
        span: Span,
    ) -> impl std::future::Future<Output = Result<Value, LabeledError>> + Send;
}
