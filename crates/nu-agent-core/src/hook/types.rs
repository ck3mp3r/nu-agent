use tokio::sync::oneshot;

/// Permission decision sent back from driver to hook
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecision {
    Allow,
    Deny,
}

/// Events sent FROM the async PromptHook TO the sync HookDriver
pub enum HookEvent {
    /// LLM completion call starting
    LlmStart,
    /// LLM completion call finished
    LlmEnd {
        response_chars: usize,
        tool_calls: usize,
        input_tokens: u64,
        output_tokens: u64,
        total_tokens: u64,
    },
    /// Tool call announced (before permission check)
    ToolStart { name: String, arguments: String },
    /// Tool execution finished
    ToolEnd {
        name: String,
        arguments: String,
        success: bool,
        result: String,
        error_kind: Option<String>,
        message: Option<String>,
    },
    /// Streaming text delta
    TextDelta { delta: String, aggregated: String },
    /// Permission request — hook blocks until driver responds via the oneshot
    AskPermission {
        tool_name: String,
        arguments: String,
        tool_call_id: Option<String>,
        responder: oneshot::Sender<PermissionDecision>,
    },
    /// Doom loop detected — informational warning
    DoomLoopDetected { tool_name: String, count: usize },
}

#[cfg(test)]
#[path = "types_test.rs"]
mod types_test;
