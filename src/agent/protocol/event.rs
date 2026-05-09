#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiEvent {
    LlmStart,
    Tick,
    LlmEnd {
        response_chars: usize,
        tool_calls: usize,
        input_tokens: u64,
        output_tokens: u64,
        total_tokens: u64,
    },
    ToolStart {
        name: String,
        source: String,
        arguments: String,
    },
    ToolEnd {
        name: String,
        source: String,
        arguments: String,
        success: bool,
        result: String,
        error_kind: Option<String>,
        message: Option<String>,
    },
    Warning {
        message: String,
    },
    CompactionTriggered {
        source: String,
        summarized_count: usize,
        kept_recent_count: usize,
        summary_preview: String,
        summary_body: String,
    },
    AssistantMessage {
        text: String,
    },
    Completed {
        tool_calls: usize,
    },
}
