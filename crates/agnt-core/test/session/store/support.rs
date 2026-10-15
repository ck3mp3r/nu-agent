//! Shared fixtures for the store test modules.
//!
//! Holds the imports and the helpers every topical store test file needs.
//! Sub-files pull them in with `use super::*;`.

pub(super) use super::super::store::{
    CompactionMarker, FsSessionStore, StoreEntry, extract_llm_context, validate_tool_call_adjacency,
};
pub(super) use crate::session::{SessionStore, extract_title};
pub(super) use crate::types::{AssistantMessage, Message};
pub(super) use chrono::Utc;
pub(super) use tempfile::TempDir;

/// Test result alias. NOTE: this module has a pre-existing test
/// (`assistant_content_rejects_tagless_block`) that uses the 2-arg
/// `Result<T, E>` form, so we cannot define a 1-arg `Result<T>` alias here.
/// New tests use the fully-qualified form instead.
pub(super) type TestResult<T> = core::result::Result<T, Box<dyn std::error::Error>>;

pub(super) fn make_tool_call_msg(id: &str) -> TestResult<crate::types::Message> {
    use crate::types::{AssistantContent, CallId, ToolCall, ToolFunction, ToolName};
    Ok(crate::types::Message::Assistant(AssistantMessage::new(
        vec![AssistantContent::ToolCall(ToolCall::new(
            CallId::from_wire(id),
            ToolFunction::new(
                ToolName::new("some_tool").map_err(|_| "non-empty tool name")?,
                serde_json::json!({}),
            ),
        ))],
    )))
}

pub(super) fn make_tool_result_msg(id: &str) -> TestResult<crate::types::Message> {
    use crate::types::{CallId, ToolName, ToolResult, ToolResultContent, UserContent};
    Ok(crate::types::Message::User {
        content: vec![UserContent::ToolResult(ToolResult {
            call: CallId::from_wire(id),
            name: ToolName::new("some_tool").map_err(|_| "non-empty tool name")?,
            content: vec![ToolResultContent::text("ok")],
            is_error: false,
        })],
    })
}
