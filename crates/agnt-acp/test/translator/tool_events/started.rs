//! Tool-call translation: `ToolStarted` and tool-kind mapping.

use agent_client_protocol::schema::v1::{SessionUpdate, ToolCallStatus, ToolKind};
use agnt_kernel::{SessionRecord, ToolCall, ToolRecord};
use agnt_render::transcript::ir::ToolName;

use super::support::*;
use crate::translator::TranslateInput;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// ToolStarted
// ---------------------------------------------------------------------------

#[test]
fn test_translator_tool_started_produces_pending_tool_call() -> Result<()> {
    // -- Setup & Fixtures
    let event = tool_started("read", "{\"path\":\"src/lib.rs\"}");

    // -- Exec
    let update = translator()
        .translate(TranslateInput::Record(&event))
        .ok_or("tool started must translate")?;

    // -- Check
    let SessionUpdate::ToolCall(call) = update else {
        return Err(format!("expected tool_call, got: {update:?}").into());
    };
    assert_eq!(call.status, ToolCallStatus::Pending);
    assert_eq!(call.kind, ToolKind::Read);
    assert_eq!(call.name.as_deref(), Some("read"));
    assert_eq!(
        call.title, "src/lib.rs",
        "the title is the call line derived from the call's own arguments"
    );
    assert!(!call.tool_call_id.0.is_empty());
    Ok(())
}

#[test]
fn test_translator_tool_started_falls_back_to_name_when_summary_empty() -> Result<()> {
    // -- Setup & Fixtures: empty arguments derive an empty call line, so the
    // name is the whole title.
    let event = SessionRecord::Tool(ToolRecord::Started {
        call: ToolCall::new(
            ToolName("glob".to_string()),
            agnt_runtime::tools::handler::ToolSource::Builtin,
            "",
        ),
    });

    // -- Exec
    let update = translator()
        .translate(TranslateInput::Record(&event))
        .ok_or("tool started must translate")?;

    // -- Check
    let SessionUpdate::ToolCall(call) = update else {
        return Err(format!("expected tool_call, got: {update:?}").into());
    };
    assert_eq!(call.title, "glob");
    Ok(())
}

#[test]
fn test_translator_tool_kind_mapping_covers_every_family() -> Result<()> {
    // -- Setup & Fixtures
    let cases = [
        ("read", ToolKind::Read),
        ("edit", ToolKind::Edit),
        ("grep", ToolKind::Search),
        ("glob", ToolKind::Search),
        ("nu", ToolKind::Execute),
        ("http", ToolKind::Fetch),
        ("skill", ToolKind::Other),
        ("ast_query", ToolKind::Read),
        ("ast_nodes", ToolKind::Read),
        ("ast_refs", ToolKind::Read),
        ("ast_tree", ToolKind::Read),
        ("tmux_session", ToolKind::Execute),
        ("tmux_window", ToolKind::Execute),
        ("tmux_pane", ToolKind::Execute),
        ("tmux_layout", ToolKind::Execute),
        ("spawn_agent", ToolKind::Other),
    ];
    let mut translator = translator();

    // -- Exec & Check
    for (name, expected) in cases {
        let update = translator
            .translate(TranslateInput::Record(&tool_started(name, "{}")))
            .ok_or_else(|| format!("tool '{name}' must translate"))?;
        let SessionUpdate::ToolCall(call) = update else {
            return Err(format!("expected tool_call for '{name}', got: {update:?}").into());
        };
        assert_eq!(call.kind, expected, "wrong kind for tool '{name}'");
    }
    Ok(())
}

#[test]
fn test_translator_tool_kind_mapping_covers_remaining_builtins() -> Result<()> {
    // -- Setup & Fixtures
    // The builtins outside the families above, and a name that is not a builtin
    // at all: both render as ACP's `Other`.
    let cases = [
        ("skill", ToolKind::Other),
        ("spawn_agent", ToolKind::Other),
        ("terminate_agent", ToolKind::Other),
        ("send_message", ToolKind::Other),
        ("list_agents", ToolKind::Other),
        ("mcp_tool", ToolKind::Other),
    ];

    // -- Exec & Check
    for (name, expected) in cases {
        assert_eq!(wire_kind(name)?, expected, "wrong kind for tool '{name}'");
    }
    Ok(())
}

#[test]
fn test_translator_tool_kind_ignores_name_prefixes() -> Result<()> {
    // -- Setup & Fixtures
    // Only a builtin's own identity classifies it: a name that merely shares a
    // builtin family's prefix (an MCP tool, a closure) is not of that family.
    let cases = [
        ("ast_foo", ToolKind::Other),
        ("tmux_custom", ToolKind::Other),
    ];

    // -- Exec & Check
    for (name, expected) in cases {
        assert_eq!(wire_kind(name)?, expected, "wrong kind for tool '{name}'");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Test Support
// ---------------------------------------------------------------------------

/// The ACP kind the client sees for a `ToolStarted` naming `name`.
fn wire_kind(name: &str) -> Result<ToolKind> {
    let update = translator()
        .translate(TranslateInput::Record(&tool_started(name, "{}")))
        .ok_or_else(|| format!("tool '{name}' must translate"))?;
    let SessionUpdate::ToolCall(call) = update else {
        return Err(format!("expected tool_call for '{name}', got: {update:?}").into());
    };
    Ok(call.kind)
}
