use nu_protocol::LabeledError;
use rig::completion::message::AssistantContent;

/// Token usage statistics from LLM response.
#[derive(Debug, Clone, PartialEq)]
pub struct LlmUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub cached_input_tokens: u64,
    pub cache_creation_input_tokens: u64,
}

impl From<rig::completion::request::Usage> for LlmUsage {
    fn from(usage: rig::completion::request::Usage) -> Self {
        Self {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            total_tokens: usage.total_tokens,
            cached_input_tokens: usage.cached_input_tokens,
            cache_creation_input_tokens: usage.cache_creation_input_tokens,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LlmResponse {
    pub text: String,
    pub usage: LlmUsage,
    pub tool_calls: Vec<AssistantContent>,
    pub tool_call_metadata: Vec<ToolCallMetadata>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCallMetadata {
    pub id: String,
    pub name: String,
    pub arguments: String,
    pub source: Option<String>,
    pub error_kind: Option<String>,
    pub message: Option<String>,
    pub details: Option<String>,
}

pub(crate) fn extract_response<T>(
    completion_response: rig::completion::CompletionResponse<T>,
) -> Result<LlmResponse, LabeledError> {
    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();

    for content in completion_response.choice {
        match content {
            AssistantContent::Text(t) => {
                text_parts.push(t.to_string());
            }
            tool_call @ AssistantContent::ToolCall(_) => {
                tool_calls.push(tool_call);
            }
            _ => {}
        }
    }

    Ok(LlmResponse {
        text: text_parts.join("\n"),
        usage: completion_response.usage.into(),
        tool_calls,
        tool_call_metadata: vec![],
    })
}
