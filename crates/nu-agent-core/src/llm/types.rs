use crate::types::AssistantContent;

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
            input_tokens: usage.input_tokens.unwrap_or(0),
            output_tokens: usage.output_tokens.unwrap_or(0),
            total_tokens: usage.total_tokens.unwrap_or(0),
            cached_input_tokens: usage.cached_input_tokens.unwrap_or(0),
            cache_creation_input_tokens: usage.cache_creation_input_tokens.unwrap_or(0),
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
