mod format;
mod types;

pub use format::format_response;
pub use types::{LlmResponse, LlmUsage, ToolCallMetadata};

// region:    --- Tests

#[cfg(test)]
#[path = "../../test/llm/format.rs"]
mod format_test;

// endregion: --- Tests
