use tokio::sync::broadcast;

/// A request to cancel the current task.
#[derive(Debug, Clone)]
pub enum CancelEvent {
    Requested { task_id: Option<String> },
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
        success: bool,
        result: String,
    },
}

/// An LLM request lifecycle event.
#[derive(Debug, Clone)]
pub enum LlmEvent {
    Start,
    End {
        input_tokens: u64,
        output_tokens: u64,
        total_tokens: u64,
    },
}

/// A conversation turn lifecycle event.
#[derive(Debug, Clone)]
pub enum TurnEvent {
    Started {
        prompt: String,
        task_id: Option<String>,
    },
    Completed {
        output: String,
        task_id: Option<String>,
    },
    Failed {
        error: String,
    },
}

/// An external (e.g. A2A) event.
#[derive(Debug, Clone)]
pub enum ExternalEvent {
    PromptReceived { prompt: String, task_id: String },
    TaskCancelled { task_id: String },
}

/// A compaction lifecycle event.
#[derive(Debug, Clone)]
pub enum CompactionEvent {
    Started,
    Completed,
}

/// A warning message.
#[derive(Debug, Clone)]
pub enum WarningEvent {
    Message { message: String },
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
