#[derive(Debug, Clone)]
pub enum UiEvent {
    LlmStart,
    Tick,
    LlmEnd {
        response_chars: usize,
        tool_calls: usize,
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
    Completed {
        tool_calls: usize,
    },
}
