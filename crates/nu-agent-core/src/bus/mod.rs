use tokio::sync::broadcast;

use crate::orchestrator::UiStateEvent;
use crate::protocol::event::{ToolDisplay, UiEvent};

/// A request to cancel the current task.
#[derive(Debug, Clone)]
pub enum CancelEvent {
    Requested,
}

/// A tool invocation lifecycle event.
#[derive(Debug, Clone)]
pub enum ToolEvent {
    Started {
        name: String,
        source: String,
        arguments: String,
    },
    Completed {
        name: String,
        source: String,
        arguments: String,
        success: bool,
        result: String,
        display: Option<ToolDisplay>,
        error_kind: Option<String>,
        message: Option<String>,
    },
}

/// An LLM request lifecycle event.
#[derive(Debug, Clone)]
pub enum LlmEvent {
    Started,
    Completed {
        response_chars: usize,
        tool_calls: usize,
        input_tokens: u64,
        output_tokens: u64,
        total_tokens: u64,
    },
    AssistantMessage {
        text: String,
    },
}

/// A conversation turn lifecycle event.
#[derive(Debug, Clone)]
pub enum TurnEvent {
    /// A turn started.
    Started {
        prompt: String,
        task_id: Option<String>,
    },
    /// A turn finished with a tool-call count for the TUI renderer.
    Completed { tool_calls: usize },
    /// An external (A2A) task completed with its output.
    TaskCompleted { output: String, task_id: String },
}

/// A session lifecycle event.
#[derive(Debug, Clone)]
pub enum SessionEvent {
    Started {
        session_id: String,
        hydrated: bool,
    },
    Ended {
        session_id: String,
    },
    Switched {
        from_session_id: Option<String>,
        to_session_id: String,
    },
}

/// An external (e.g. A2A) event.
#[derive(Debug, Clone)]
pub enum ExternalEvent {
    PromptReceived { prompt: String, task_id: String },
}

/// A compaction lifecycle event.
#[derive(Debug, Clone)]
pub enum CompactionEvent {
    /// A compaction has been requested (auto-threshold or `/compact`). The
    /// orchestrator is the single subscriber that acts on this; the TUI does
    /// not render it.
    Requested {
        source: String,
    },
    Started {
        source: String,
    },
    SummaryChunk {
        source: String,
        delta: String,
        aggregated: String,
    },
    Completed {
        source: String,
        summary_preview: String,
        summary_body: String,
    },
    Failed {
        source: String,
        message: String,
    },
}

/// A warning message.
#[derive(Debug, Clone)]
pub enum WarningEvent {
    Message { message: String },
    TurnError { message: String },
}

/// A permission lifecycle event.
#[derive(Debug, Clone)]
pub enum PermissionEvent {
    Requested {
        request_id: String,
        context: Box<crate::protocol::event::PermissionRequestContext>,
    },
    DecisionSubmitted {
        request_id: String,
        decision: crate::protocol::event::PermissionDecision,
        matched_rule_identity: String,
    },
    DecisionTimedOut {
        request_id: String,
    },
    DecisionIgnored {
        request_id: String,
        reason: String,
    },
}

impl From<ToolEvent> for Option<UiEvent> {
    fn from(event: ToolEvent) -> Self {
        match event {
            ToolEvent::Started {
                name,
                source,
                arguments,
            } => Some(UiEvent::ToolStarted {
                name,
                source,
                arguments,
            }),
            ToolEvent::Completed {
                name,
                source,
                arguments,
                success,
                result,
                display,
                error_kind,
                message,
            } => Some(UiEvent::ToolCompleted {
                name,
                source,
                arguments,
                success,
                result,
                display,
                error_kind,
                message,
            }),
        }
    }
}

impl From<LlmEvent> for Option<UiEvent> {
    fn from(event: LlmEvent) -> Self {
        match event {
            LlmEvent::Started => Some(UiEvent::LlmStarted),
            LlmEvent::Completed {
                response_chars,
                tool_calls,
                input_tokens,
                output_tokens,
                total_tokens,
            } => Some(UiEvent::LlmCompleted {
                response_chars,
                tool_calls,
                input_tokens,
                output_tokens,
                total_tokens,
            }),
            LlmEvent::AssistantMessage { text } => Some(UiEvent::AssistantMessage { text }),
        }
    }
}

impl From<WarningEvent> for Option<UiEvent> {
    fn from(event: WarningEvent) -> Self {
        match event {
            WarningEvent::Message { message } => Some(UiEvent::Warning { message }),
            WarningEvent::TurnError { message } => Some(UiEvent::TurnError { message }),
        }
    }
}

impl From<CompactionEvent> for Option<UiEvent> {
    fn from(event: CompactionEvent) -> Self {
        match event {
            CompactionEvent::Requested { .. } => None,
            CompactionEvent::Started { source } => Some(UiEvent::CompactionStarted { source }),
            CompactionEvent::SummaryChunk {
                source,
                delta,
                aggregated,
            } => Some(UiEvent::CompactionSummaryChunk {
                source,
                delta,
                aggregated,
            }),
            CompactionEvent::Completed {
                source,
                summary_preview,
                summary_body,
            } => Some(UiEvent::CompactionCompleted {
                source,
                summary_preview,
                summary_body,
            }),
            CompactionEvent::Failed { source, message } => {
                Some(UiEvent::CompactionFailed { source, message })
            }
        }
    }
}

impl From<TurnEvent> for Option<UiEvent> {
    fn from(event: TurnEvent) -> Self {
        match event {
            // Turn started is not rendered in the TUI.
            TurnEvent::Started { .. } => None,
            TurnEvent::Completed { tool_calls } => Some(UiEvent::Completed { tool_calls }),
            // A2A-only completion — not for the TUI.
            TurnEvent::TaskCompleted { .. } => None,
        }
    }
}

impl From<SessionEvent> for Option<UiEvent> {
    fn from(_event: SessionEvent) -> Self {
        // Session lifecycle events are not rendered in the TUI.
        None
    }
}

impl From<PermissionEvent> for Option<UiEvent> {
    fn from(event: PermissionEvent) -> Self {
        match event {
            PermissionEvent::Requested {
                request_id,
                context,
            } => Some(UiEvent::PermissionRequested {
                request_id,
                context: *context,
            }),
            PermissionEvent::DecisionSubmitted {
                request_id,
                decision,
                matched_rule_identity,
            } => Some(UiEvent::PermissionDecisionSubmitted {
                request_id,
                decision,
                matched_rule_identity,
            }),
            PermissionEvent::DecisionTimedOut { request_id } => {
                Some(UiEvent::PermissionDecisionTimedOut { request_id })
            }
            PermissionEvent::DecisionIgnored { request_id, reason } => {
                Some(UiEvent::PermissionDecisionIgnored { request_id, reason })
            }
        }
    }
}

/// Typed broadcast channels, one per event category.
///
/// Each channel carries its own event type, so the compiler enforces that,
/// for example, a `ToolEvent` can never be sent on the cancel channel.
#[derive(Clone)]
pub struct Bus {
    cancel: broadcast::Sender<CancelEvent>,
    tool: broadcast::Sender<ToolEvent>,
    llm: broadcast::Sender<LlmEvent>,
    turn: broadcast::Sender<TurnEvent>,
    session: broadcast::Sender<SessionEvent>,
    external: broadcast::Sender<ExternalEvent>,
    compaction: broadcast::Sender<CompactionEvent>,
    warning: broadcast::Sender<WarningEvent>,
    permission: broadcast::Sender<PermissionEvent>,
    ui_state: broadcast::Sender<UiStateEvent>,
}

impl Bus {
    pub fn new() -> Self {
        Self {
            cancel: broadcast::channel(64).0,
            tool: broadcast::channel(256).0,
            llm: broadcast::channel(64).0,
            turn: broadcast::channel(64).0,
            session: broadcast::channel(16).0,
            external: broadcast::channel(64).0,
            compaction: broadcast::channel(16).0,
            warning: broadcast::channel(64).0,
            permission: broadcast::channel(64).0,
            ui_state: broadcast::channel(64).0,
        }
    }

    pub fn cancel(&self) -> &broadcast::Sender<CancelEvent> {
        &self.cancel
    }
    pub fn tool(&self) -> &broadcast::Sender<ToolEvent> {
        &self.tool
    }
    pub fn llm(&self) -> &broadcast::Sender<LlmEvent> {
        &self.llm
    }
    pub fn turn(&self) -> &broadcast::Sender<TurnEvent> {
        &self.turn
    }
    pub fn session(&self) -> &broadcast::Sender<SessionEvent> {
        &self.session
    }
    pub fn external(&self) -> &broadcast::Sender<ExternalEvent> {
        &self.external
    }
    pub fn compaction(&self) -> &broadcast::Sender<CompactionEvent> {
        &self.compaction
    }
    pub fn warning(&self) -> &broadcast::Sender<WarningEvent> {
        &self.warning
    }
    pub fn permission(&self) -> &broadcast::Sender<PermissionEvent> {
        &self.permission
    }
    pub fn ui_state(&self) -> &broadcast::Sender<UiStateEvent> {
        &self.ui_state
    }
}

impl Default for Bus {
    fn default() -> Self {
        Self::new()
    }
}

pub fn create_bus() -> Bus {
    Bus::new()
}

#[cfg(test)]
#[path = "event_from_test.rs"]
mod event_from_test;
#[cfg(test)]
mod test;
