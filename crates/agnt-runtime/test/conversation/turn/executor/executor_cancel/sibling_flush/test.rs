//! The cancel arm's flush of a batch's undispatched siblings.
//!
//! A stop drops the stream before the batch's later calls dispatch, so nothing
//! emitted their pair. The executor's cancel arm flushes each one as a denied
//! pair, in commit order, so the live transcript renders it.

// region:    --- Modules

#[path = "denied_pairs.rs"]
mod denied_pairs;

// endregion: --- Modules

use super::*;

use crate::conversation::turn::executor::test_utils::TurnTestObservation;
use agnt_kernel::{SessionRecord, ToolErrorKind, ToolRecord, ToolStatus};
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

type ToolEventSummary = (
    String,
    Option<ToolStatus>,
    Option<ToolErrorKind>,
    Option<String>,
);

/// The tool-event shape the flush assertions compare: `(name, status,
/// error_kind, message)`, with the last three `None` for a `Started` row.
fn tool_event_summary(event: &TurnTestObservation) -> Option<ToolEventSummary> {
    match event {
        TurnTestObservation::Record(SessionRecord::Tool(ToolRecord::Started { call })) => {
            Some((call.name.0.clone(), None, None, None))
        }
        TurnTestObservation::Record(SessionRecord::Tool(ToolRecord::Completed {
            call,
            outcome,
        })) => Some((
            call.name.0.clone(),
            Some(outcome.status),
            outcome.kind,
            outcome.message.clone(),
        )),
        _ => None,
    }
}

/// The tool definitions the flush tests advertise for the two test tools.
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

// endregion: --- Support
