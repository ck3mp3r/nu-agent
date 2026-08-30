use nu_protocol::{LabeledError, Span, Value};

use crate::bus::Bus;
use crate::orchestrator::OrchestratorEvent;
use crate::protocol::event::{ToolDisplay, UiEvent};

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
}

pub trait UserInputUi {
    fn event_sender(&self) -> &tokio::sync::mpsc::Sender<OrchestratorEvent>;
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
    fn execute_turn(
        &mut self,
        bus: &Bus,
        prompt: String,
        context: Option<String>,
        span: Span,
    ) -> impl std::future::Future<Output = Result<Value, LabeledError>> + Send;
}
