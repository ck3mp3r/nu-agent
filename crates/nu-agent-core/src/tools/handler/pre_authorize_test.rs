use std::path::PathBuf;

use nu_protocol::Span;
use serde_json::json;

use super::*;
use crate::types::{ToolCall, ToolCallId, ToolFunction};

// Test-support nu_plugin::EngineInterface implementation. The real
// EngineInterface cannot be constructed outside plugin context (see
// tools/executor_test.rs for the in-tree precedent), so tests supply their
// own `engine: EngineInterfaceLike` value. Production code is unaffected:
// `EngineInterfaceLike` is blanket-impl'd for the real `EngineInterface`
// and the functions take `impl` generics.
struct TestEngine {
    cwd: PathBuf,
}

impl EngineInterfaceLike for TestEngine {
    fn get_span_contents(&self, _span: Span) -> core::result::Result<Vec<u8>, String> {
        Err("span contents not needed by pre-authorize tests".to_string())
    }

    fn get_current_dir(&self) -> core::result::Result<String, String> {
        Ok(self.cwd.to_string_lossy().to_string())
    }
}

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

fn make_tool_call(name: &str, arguments: serde_json::Value) -> ToolCall {
    ToolCall::new(
        ToolCallId::new_or_mint("test-id"),
        ToolFunction::new(name.to_string(), arguments),
    )
}

#[test]
fn test_pre_authorize_tool_call_nu_with_string_command_defaults() -> Result<()> {
    // -- Setup & Fixtures
    let tool_call = make_tool_call("nu", json!({"command": "ls | where size > 1mb"}));

    // -- Exec
    let output = pre_authorize_tool_call(
        &tool_call,
        ToolSource::Builtin,
        &TestEngine {
            cwd: PathBuf::from("/tmp"),
        },
    );

    // -- Check
    // The nu preview block was removed (task 6a581540): the nu command is
    // rendered highlighted in the tool status row instead, so pre-authorize
    // must return the default output for nu.
    assert!(
        output.display.is_none(),
        "nu must not produce a pre-authorize display"
    );
    assert!(
        output.ask_context.pre_authorize_display.is_none(),
        "nu must not populate ask_context.pre_authorize_display"
    );
    Ok(())
}

#[test]
fn test_pre_authorize_tool_call_nu_missing_command_defaults() -> Result<()> {
    // -- Setup & Fixtures
    let tool_call = make_tool_call("nu", json!({}));

    // -- Exec
    let output = pre_authorize_tool_call(
        &tool_call,
        ToolSource::Builtin,
        &TestEngine {
            cwd: PathBuf::from("/tmp"),
        },
    );

    // -- Check
    assert!(
        output.display.is_none(),
        "missing command should return default output"
    );
    assert!(
        output.ask_context.pre_authorize_display.is_none(),
        "missing command should leave ask_context empty"
    );
    Ok(())
}

#[test]
fn test_pre_authorize_tool_call_nu_non_string_command_defaults() -> Result<()> {
    // -- Setup & Fixtures
    let tool_call = make_tool_call("nu", json!({"command": 42}));

    // -- Exec
    let output = pre_authorize_tool_call(
        &tool_call,
        ToolSource::Builtin,
        &TestEngine {
            cwd: PathBuf::from("/tmp"),
        },
    );

    // -- Check
    assert!(
        output.display.is_none(),
        "non-string command should return default output"
    );
    assert!(
        output.ask_context.pre_authorize_display.is_none(),
        "non-string command should leave ask_context empty"
    );
    Ok(())
}

#[test]
fn test_pre_authorize_tool_call_edit_path_unchanged_missing_command() -> Result<()> {
    // The edit path goes through pre_authorize_fs_tool, which returns None
    // when the arguments do not form a valid apply plan. These arguments
    // fail validation (search_replace without operation.search), so the
    // unchanged None → default path runs without touching disk.
    // -- Setup & Fixtures
    let tool_call = make_tool_call("edit", json!({"path": "nonexistent-file.txt"}));

    // -- Exec
    let output = pre_authorize_tool_call(
        &tool_call,
        ToolSource::Builtin,
        &TestEngine {
            cwd: PathBuf::from("/tmp"),
        },
    );

    // -- Check
    assert!(
        output.display.is_none(),
        "edit path behavior must stay unchanged (no preview for a non-applicable edit)"
    );
    assert!(output.ask_context.pre_authorize_display.is_none());
    Ok(())
}

#[test]
fn test_pre_authorize_tool_call_mcp_source_defaults() -> Result<()> {
    // -- Setup & Fixtures
    let tool_call = make_tool_call("nu", json!({"command": "ls"}));

    // -- Exec
    let output = pre_authorize_tool_call(
        &tool_call,
        ToolSource::Mcp,
        &TestEngine {
            cwd: PathBuf::from("/tmp"),
        },
    );

    // -- Check
    assert!(output.display.is_none(), "mcp source should return default");
    assert!(output.ask_context.pre_authorize_display.is_none());
    Ok(())
}

#[test]
fn test_pre_authorize_tool_call_unknown_source_defaults() -> Result<()> {
    // -- Setup & Fixtures
    let tool_call = make_tool_call("nu", json!({"command": "ls"}));

    // -- Exec
    let output = pre_authorize_tool_call(
        &tool_call,
        ToolSource::Unknown,
        &TestEngine {
            cwd: PathBuf::from("/tmp"),
        },
    );

    // -- Check
    assert!(
        output.display.is_none(),
        "unknown source should return default"
    );
    assert!(output.ask_context.pre_authorize_display.is_none());
    Ok(())
}

#[test]
fn test_pre_authorize_fs_tool_edit_apply_produces_diff_preview() -> Result<()> {
    // Regression guard (task 6a581540): removing the nu preview must not
    // touch the edit preview path. A valid create-mode edit produces a
    // pre-authorize diff ToolDisplay in both slots.
    // -- Setup & Fixtures
    let tmp = tempfile::tempdir()?;
    let target = tmp.path().join("new-file.txt");
    let tool_call = make_tool_call(
        "edit",
        json!({
            "path": target.to_string_lossy(),
            "operation": {"type": "create", "content": "hello\n"}
        }),
    );

    // -- Exec
    let output = pre_authorize_tool_call(
        &tool_call,
        ToolSource::Builtin,
        &TestEngine {
            cwd: tmp.path().to_path_buf(),
        },
    );

    // -- Check
    let display = output
        .display
        .ok_or("edit apply must produce a pre-authorize display")?;
    let section = display.sections.first().ok_or("should have one section")?;
    assert_eq!(section.language, "diff");
    assert!(
        section.content.contains("+hello"),
        "diff must contain the created content, got: {:?}",
        section.content
    );
    let ask_display = output
        .ask_context
        .pre_authorize_display
        .ok_or("edit apply must populate ask_context.pre_authorize_display")?;
    assert_eq!(ask_display, display);
    Ok(())
}
