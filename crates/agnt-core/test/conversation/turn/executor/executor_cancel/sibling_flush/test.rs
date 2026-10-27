//! The cancel arm's flush of a batch's undispatched siblings.
//!
//! A stop drops the stream before the batch's later calls dispatch, so nothing
//! emitted their pair. The executor's cancel arm flushes each one as a denied
//! pair, in commit order, so the live transcript renders it.

// region:    --- Modules

#[path = "denied_pairs.rs"]
mod denied_pairs;

#[path = "preview.rs"]
mod preview;

// endregion: --- Modules

use super::*;

use rig::tool::Tool;

// region:    --- Support

/// A tool that returns a fixed result, so a batch's later calls have a normal
/// target.
struct EchoTool;

impl rig::tool::Tool for EchoTool {
    const NAME: &'static str = "echo_tool";
    type Error = std::convert::Infallible;
    type Args = serde_json::Value;
    type Output = String;

    fn description(&self) -> String {
        "Echoes a fixed result".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {"x": {"type": "number"}}})
    }

    async fn call(
        &self,
        _context: &mut rig::tool::ToolContext,
        _args: Self::Args,
    ) -> std::result::Result<Self::Output, Self::Error> {
        Ok("echo_result".to_string())
    }
}

type ToolEventSummary = (String, Option<bool>, Option<String>, Option<String>);

/// The tool-event shape the flush assertions compare: `(name, success,
/// error_kind, message)`, with the last three `None` for a `ToolStarted`.
fn tool_event_summary(event: &UiEvent) -> Option<ToolEventSummary> {
    match event {
        UiEvent::ToolStarted { name, .. } => Some((name.clone(), None, None, None)),
        UiEvent::ToolCompleted {
            name,
            success,
            error_kind,
            message,
            ..
        } => Some((
            name.clone(),
            Some(*success),
            error_kind.clone(),
            message.clone(),
        )),
        _ => None,
    }
}

/// The tool definitions the executor advertises for the two test tools.
fn test_tool_definitions() -> Result<Vec<crate::types::ToolDefinition>> {
    Ok(vec![
        crate::types::ToolDefinition {
            annotations: crate::types::ToolAnnotations::default(),
            name: crate::types::ToolName::new("test_cancel_tool")?,
            description: "cancels the turn".to_string(),
            parameters: serde_json::json!({"type": "object", "properties": {}}),
        },
        crate::types::ToolDefinition {
            annotations: crate::types::ToolAnnotations::default(),
            name: crate::types::ToolName::new(EchoTool::NAME)?,
            description: "Echoes a fixed result".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {"x": {"type": "number"}}
            }),
        },
    ])
}

/// The tool definitions the preview-sequence test advertises: the two test
/// tools plus `nu`, which the stream calls but the stop leaves undispatched.
/// The definition keeps rig's visitor from raising `UnknownToolCall`.
fn test_tool_definitions_with_nu() -> Result<Vec<crate::types::ToolDefinition>> {
    let mut definitions = test_tool_definitions()?;
    definitions.push(crate::types::ToolDefinition {
        annotations: crate::types::ToolAnnotations::default(),
        name: crate::types::ToolName::new("nu")?,
        description: "Runs a nu command".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {"command": {"type": "string"}}
        }),
    });
    Ok(definitions)
}

/// The event shape the preview-sequence assertion compares.
#[derive(Debug, PartialEq, Eq)]
enum FlushEvent {
    Started(String),
    Preview {
        tool_key: String,
        content: String,
        language: String,
    },
    Completed {
        name: String,
        success: bool,
        error_kind: Option<String>,
    },
}

/// Project a UI event onto the sequence shape; `None` for events the sequence
/// does not assert.
fn flush_event(event: &UiEvent) -> Option<FlushEvent> {
    match event {
        UiEvent::ToolStarted { name, .. } => Some(FlushEvent::Started(name.clone())),
        UiEvent::ToolPreview { tool_key, display } => {
            let section = display.sections.first()?;
            let language = match &section.kind {
                crate::transcript::ir::ContentKind::Code { language } => language.clone(),
                _ => String::new(),
            };
            Some(FlushEvent::Preview {
                tool_key: tool_key.clone(),
                content: section.content.clone(),
                language,
            })
        }
        UiEvent::ToolCompleted {
            name,
            success,
            error_kind,
            ..
        } => Some(FlushEvent::Completed {
            name: name.clone(),
            success: *success,
            error_kind: error_kind.clone(),
        }),
        _ => None,
    }
}

// endregion: --- Support
