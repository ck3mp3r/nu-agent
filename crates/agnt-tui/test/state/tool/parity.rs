use super::*;

use crate::runtime::RuntimeCoordinator;
use agnt_core::session::resolver::hydrate_transcript_messages;
use agnt_core::tools::handler::builtin_tool::call_line_render_for;
use agnt_core::tools::handler::tool_display_from_arguments;
use agnt_core::transcript::ir::Block;
use agnt_core::types::{
    AssistantContent, AssistantMessage, CallId, Message, Text, ToolCall, ToolFunction, ToolName,
    ToolResult, ToolResultContent, UserContent,
};

/// The not-run render contract spans two planes: the live event sequence
/// (`ToolStarted` → `ToolPreview` → `ToolCompleted` with a not-run kind) and
/// the persisted rows replayed through the resolver. Both must produce the
/// same block list — same length, same per-index text, status, and fill, in
/// the same order — for both not-run kinds.
#[test]
fn test_tool_parity_not_run_live_and_rehydrate_block_lists_match() -> Result<()> {
    // -- Setup & Fixtures
    let temp_dir = tempfile::TempDir::new()?;
    let target = temp_dir.path().join("target.rs");
    let pre_edit = "fn main() {\n    let old = 1;\n}\n";
    std::fs::write(&target, pre_edit)?;
    let target_path = target
        .to_str()
        .ok_or("temp path should be utf-8")?
        .to_string();
    let arguments = format!(
        r#"{{"path":"{target_path}","mode":"apply","operation":{{"type":"search_replace","search":"let old = 1;","replacement":"let new = 2;"}}}}"#
    );
    let cwd = temp_dir.path();
    // (kind, the persisted result text the row carries, the live message)
    let cases = [
        (
            "cancelled",
            "No result provided",
            agnt_core::hook::chain::CANCELLED_DISPATCH_MESSAGE,
        ),
        (
            "denied",
            "Permission denied by rule 'global:*' (scope: global)",
            "Permission denied by rule 'global:*' (scope: global)",
        ),
    ];

    // -- Exec & Check
    for (kind, row_text, message) in cases {
        let live = live_blocks(&arguments, cwd, kind, message)?;
        let rehydrated = rehydrated_blocks(&arguments, cwd, row_text)?;

        assert_eq!(
            block_projection(&live),
            block_projection(&rehydrated),
            "{kind}: the live and rehydrated block lists must match"
        );
        assert_eq!(
            live.len(),
            2,
            "{kind}: the tool block plus its args preview"
        );
        assert_eq!(
            live[0].status,
            Some(ItemStatus::Cancelled),
            "{kind}: the tool block renders the not-run class"
        );
        assert!(
            !live
                .iter()
                .any(|block| block.source.plain_text().contains(message)),
            "{kind}: the not-run message is not rendered"
        );
    }

    // -- Check: the call never ran, so the target keeps its pre-edit state.
    assert_eq!(
        std::fs::read_to_string(&target)?,
        pre_edit,
        "the fixture file must stay in its pre-edit state"
    );
    Ok(())
}

// region:    --- Test Support

/// Project a block list to the tuple the parity assertion compares: the
/// flattened text, the status, and the fill, in block order.
fn block_projection(blocks: &[Block]) -> Vec<(String, Option<ItemStatus>, Fill)> {
    blocks
        .iter()
        .map(|block| (block.source.plain_text(), block.status, block.fill))
        .collect()
}

/// Drive the live plane: the three events the hook emitters publish for a call
/// the turn never ran.
fn live_blocks(
    arguments: &str,
    cwd: &std::path::Path,
    kind: &str,
    message: &str,
) -> Result<Vec<Block>> {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.reduce_ui_event(UiEvent::ToolStarted {
        name: "edit".to_string(),
        source: "builtin".to_string(),
        arguments: arguments.to_string(),
        call_line: call_line_render_for("edit", arguments),
    });
    let display = tool_display_from_arguments("edit", arguments, cwd)
        .ok_or("the edit call should have an args preview")?;
    coordinator.reduce_ui_event(UiEvent::ToolPreview {
        tool_key: format!("edit\n{arguments}"),
        display,
    });
    coordinator.reduce_ui_event(UiEvent::ToolCompleted {
        name: "edit".to_string(),
        source: "builtin".to_string(),
        arguments: arguments.to_string(),
        success: false,
        result: String::new(),
        display: None,
        error_kind: Some(kind.to_string()),
        message: Some(message.to_string()),
    });
    Ok(coordinator.state().transcript.blocks().to_vec())
}

/// Drive the rehydration plane: the persisted rows through the resolver, then
/// the coordinator's transcript hydration.
fn rehydrated_blocks(arguments: &str, cwd: &std::path::Path, row_text: &str) -> Result<Vec<Block>> {
    let tool_name = ToolName::new("edit").map_err(|_| "should be a non-empty tool name")?;
    let rows = vec![
        Message::Assistant(AssistantMessage::new(vec![AssistantContent::ToolCall(
            ToolCall::new(
                CallId::from_wire("call_parity"),
                ToolFunction::parse(tool_name.clone(), arguments),
            ),
        )])),
        Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: CallId::from_wire("call_parity"),
                name: tool_name,
                content: vec![ToolResultContent::Text(Text::new(row_text))],
                is_error: true,
            })],
        },
    ];
    let snapshots = hydrate_transcript_messages(&rows, cwd);
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(snapshots, None);
    Ok(coordinator.state().transcript.blocks().to_vec())
}

// endregion: --- Test Support
