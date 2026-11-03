use nu_protocol::{LabeledError, Span, Value};

use crate::bus::Bus;
use crate::orchestrator::OrchestratorEvent;
use crate::protocol::events::SystemEvent;
use crate::session::tool_status::ToolStatus;

/// The usability of one MCP server — re-exported from `agnt_runtime::protocol`
/// while the runtime layer is strangled out of this crate.
pub use agnt_runtime::protocol::McpUsabilityState;

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
    /// The call's structural status. A row the resolver reads carries the
    /// variant the store wrote; a row built without one carries the verdict its
    /// `tool_success` states.
    tool_status: Option<ToolStatus>,
    /// The submit instant the entry's turn recorded, when the entry is a user
    /// row whose key joined a settled turn. The recorded value, never a
    /// re-derivation.
    stamp_millis: Option<i64>,
    /// The model identity the entry's turn ran under.
    stamp_model: Option<String>,
    /// The persona identity that answered the entry's turn.
    stamp_agent: Option<String>,
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
            tool_status: None,
            stamp_millis: None,
            stamp_model: None,
            stamp_agent: None,
            usage: None,
        }
    }

    /// Mark the snapshot with the footer facts its entry carries: the submit
    /// instant and the identities the turn ran under, verbatim.
    pub fn with_stamp(
        mut self,
        millis: Option<i64>,
        model: Option<String>,
        agent: Option<String>,
    ) -> Self {
        self.stamp_millis = millis;
        self.stamp_model = model;
        self.stamp_agent = agent;
        self
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
        // The verdict the row states is the boolean fallback: a row the
        // store records a status for overrides it with `with_tool_status`.
        self.tool_status = success.map(|success| {
            if success {
                ToolStatus::Success
            } else {
                ToolStatus::Failed
            }
        });
        self
    }

    pub fn with_tool_name(mut self, name: String) -> Self {
        self.tool_name = Some(name);
        self
    }

    /// Mark the snapshot with the call's structural status: the variant the
    /// store carries for it.
    pub fn with_tool_status(mut self, status: ToolStatus) -> Self {
        self.tool_status = Some(status);
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

    /// The call's structural status, when the row carries one.
    pub fn tool_status(&self) -> Option<ToolStatus> {
        self.tool_status
    }

    pub fn usage(&self) -> Option<UiMessageUsageSnapshot> {
        self.usage
    }

    /// The recorded submit instant the row carries, when it carries one.
    pub fn stamp_millis(&self) -> Option<i64> {
        self.stamp_millis
    }

    /// The model identity the row carries, when it carries one.
    pub fn stamp_model(&self) -> Option<&str> {
        self.stamp_model.as_deref()
    }

    /// The persona identity the row carries, when it carries one.
    pub fn stamp_agent(&self) -> Option<&str> {
        self.stamp_agent.as_deref()
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
    fn emit(&mut self, event: &SystemEvent);
    fn flush(&mut self);
    fn take_cancel_requested(&self) -> bool;
    fn emit_batch(&mut self, events: &[SystemEvent]) {
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
