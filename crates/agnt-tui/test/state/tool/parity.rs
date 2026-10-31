use super::*;

use crate::runtime::RuntimeCoordinator;
use agnt_core::session::resolver::hydrate_transcript_messages;
use agnt_core::session::tool_status::{ToolStatus, ToolStatusMap};
use agnt_core::tools::handler::builtin_tool::call_line_render_for;
use agnt_core::tools::handler::tool_display_from_arguments;
use agnt_core::transcript::entry::TranscriptEntry;
use agnt_core::types::{
    AssistantContent, AssistantMessage, CallId, Message, Text, ToolCall, ToolFunction, ToolName,
    ToolResult, ToolResultContent, UserContent,
};

/// The determinism pin, per variant: a call's row has one shape whether the
/// live event plane finished it or the store replayed it. For each of the
/// three variants the live sequence (`ToolStarted` → `ToolCompleted`) and the
/// rehydrated rows yield the same entry list — same length, same per-index
/// text, status, and fill, in the same order.
#[test]
fn test_tool_parity_live_and_rehydrate_entry_lists_match_per_variant() -> Result<()> {
    // -- Setup & Fixtures
    let cwd = std::path::Path::new(".");
    let arguments = r#"{"path":"/tmp/parity.txt"}"#;
    let cases = [
        ("success", ToolStatus::Success, ItemStatus::Done),
        ("failed", ToolStatus::Failed, ItemStatus::Failed),
        ("cancelled", ToolStatus::Cancelled, ItemStatus::Cancelled),
    ];

    // -- Exec & Check
    for (label, status, expected) in cases {
        let live = live_entries("read", arguments, cwd, status, None)?;
        let rehydrated = rehydrated_entries("read", arguments, cwd, "", status)?;

        assert_eq!(
            entry_projection(&live),
            entry_projection(&rehydrated),
            "{label}: the live and rehydrated entry lists must match"
        );
        assert_eq!(live.len(), 1, "{label}: one tool entry");
        assert_eq!(entry_status(&live[0]), Some(expected), "{label}");
    }
    Ok(())
}

/// The cancelled class spans three persisted shapes — a wall-clock kill, the
/// user's cancel, and a call that never ran — and the store records one variant
/// for all of them. Each replays as the same ⊘ row carrying the command preview
/// the live plane pushes, which is the determinism property
/// `reduce(replay(log)) == reduce(log)` for the class.
#[test]
fn test_tool_parity_cancelled_nu_shapes_rehydrate_as_one_row() -> Result<()> {
    // -- Setup & Fixtures: the rows verbatim as the store holds them. The texts
    // differ; the status the store records does not.
    let cwd = std::path::Path::new(".");
    let arguments = r#"{"command":"ls | sort"}"#;
    let rows = [
        (
            "nu timeout kill",
            "command timed out after 120 seconds: no details",
        ),
        ("nu user cancel", "nu command cancelled by user: no details"),
        ("never started", "[no result provided]"),
    ];

    // -- Exec & Check
    let live = live_entries(
        "nu",
        arguments,
        cwd,
        ToolStatus::Cancelled,
        Some("the turn stopped before this call ran"),
    )?;
    for (label, row_text) in rows {
        let rehydrated = rehydrated_entries("nu", arguments, cwd, row_text, ToolStatus::Cancelled)?;

        assert_eq!(
            entry_projection(&live),
            entry_projection(&rehydrated),
            "{label}: the live and rehydrated entry lists must match"
        );
        assert_eq!(live.len(), 1, "{label}: one tool entry");
        assert_eq!(
            entry_status(&live[0]),
            Some(ItemStatus::Cancelled),
            "{label}: the cancelled class renders ⊘"
        );
        let TranscriptEntry::Tool(tool) = &live[0] else {
            return Err(format!("{label}: expected a Tool entry").into());
        };
        let preview = tool
            .preview
            .as_ref()
            .ok_or(format!("{label}: the command preview is docked on the row"))?;
        assert_eq!(
            display_lines(preview),
            "ls | sort",
            "{label}: the preview carries the nu command"
        );
    }
    Ok(())
}
// region:    --- Test Support

/// Project an entry list to the tuple the parity assertion compares: the
/// flattened text and the status, in entry order.
fn entry_projection(entries: &[TranscriptEntry]) -> Vec<(String, Option<ItemStatus>)> {
    entries
        .iter()
        .map(|entry| (entry_text(entry), entry_status(entry)))
        .collect()
}

/// The status one entry carries, matching the block shape's `status` field: a
/// tool entry reports its own status, every other family reports `None`.
fn entry_status(entry: &TranscriptEntry) -> Option<ItemStatus> {
    match entry {
        TranscriptEntry::Tool(tool) => Some(tool.status),
        _ => None,
    }
}

/// Drive the live plane: the events the hook emitters publish for a call, closed
/// with `status`.
fn live_entries(
    tool: &str,
    arguments: &str,
    cwd: &std::path::Path,
    status: ToolStatus,
    message: Option<&str>,
) -> Result<Vec<TranscriptEntry>> {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    let call = || {
        agnt_core::protocol::events::ToolCall::new(
            agnt_core::transcript::ir::ToolName(tool.to_string()),
            "builtin",
            arguments,
            call_line_render_for(tool, arguments),
        )
    };
    coordinator.reduce_system_event(SystemEvent::ToolStarted { call: call() });
    if let Some(display) = tool_display_from_arguments(tool, arguments, cwd) {
        coordinator.reduce_system_event(SystemEvent::PreviewRequested {
            call: call(),
            display,
        });
    }
    coordinator.reduce_system_event(SystemEvent::ToolCompleted {
        call: call(),
        outcome: agnt_core::protocol::events::ToolOutcome {
            success: matches!(status, ToolStatus::Success),
            status,
            result: String::new(),
            display: None,
            error_kind: None,
            message: message.map(str::to_string),
        },
    });
    Ok(coordinator.state().transcript.entries().to_vec())
}

/// Drive the rehydration plane: the persisted rows, read with the status the
/// store carries for them, then the coordinator's transcript hydration.
fn rehydrated_entries(
    tool: &str,
    arguments: &str,
    cwd: &std::path::Path,
    row_text: &str,
    status: ToolStatus,
) -> Result<Vec<TranscriptEntry>> {
    let tool_name = ToolName::new(tool).map_err(|_| "should be a non-empty tool name")?;
    let call = CallId::from_wire("call_parity");
    let rows = vec![
        Message::Assistant(AssistantMessage::new(vec![AssistantContent::ToolCall(
            ToolCall::new(
                call.clone(),
                ToolFunction::parse(tool_name.clone(), arguments),
            ),
        )])),
        Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: call.clone(),
                name: tool_name,
                content: vec![ToolResultContent::Text(Text::new(row_text))],
                is_error: !matches!(status, ToolStatus::Success),
            })],
        },
    ];
    let mut statuses = ToolStatusMap::new();
    statuses.insert(call, status);
    let snapshots = hydrate_transcript_messages(&rows, cwd, &statuses);
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(snapshots, None);
    Ok(coordinator.state().transcript.entries().to_vec())
}

// endregion: --- Test Support
