use tokio::sync::broadcast;

use crate::protocol::event::ToolDisplay;

/// A request to cancel the current task.
#[derive(Debug, Clone)]
pub enum CancelEvent {
    Requested,
}

/// A tool invocation lifecycle event.
#[derive(Debug, Clone)]
pub enum ToolEvent {
    Start {
        name: String,
        source: String,
        arguments: String,
    },
    End {
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
    Start,
    End {
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
    TurnCompleted { tool_calls: usize },
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
    Started {
        source: Option<String>,
    },
    SummaryChunk {
        source: String,
        delta: String,
        aggregated: String,
    },
    Triggered {
        source: String,
        summarized_count: usize,
        kept_recent_count: usize,
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
mod test;
