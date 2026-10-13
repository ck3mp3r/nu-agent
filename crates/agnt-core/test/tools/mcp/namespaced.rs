use super::*;
use crate::tools::limits::MAX_TOOL_OUTPUT_BYTES;
use crate::types::{EmptyToolName, ToolName};
use rig::tool::{DynamicTool, ToolOutput};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Build a mock DynamicTool for testing
fn mock_tool(name: &str, description: &str) -> core::result::Result<DynamicTool, EmptyToolName> {
    let name = ToolName::new(name)?;
    let description = description.to_string();
    Ok(DynamicTool::new(
        name,
        description,
        serde_json::json!({}),
        |_args| Box::pin(async { Ok(ToolOutput::text("mock result")) }),
    ))
}

/// Build a mock DynamicTool that returns a large result exceeding MAX_TOOL_OUTPUT_BYTES.
fn large_mock_tool() -> core::result::Result<DynamicTool, EmptyToolName> {
    Ok(DynamicTool::new(
        ToolName::new("big_tool")?,
        "Returns lots of data",
        serde_json::json!({}),
        |_args| {
            let large = "x".repeat(MAX_TOOL_OUTPUT_BYTES + 1_000);
            Box::pin(async move { Ok(ToolOutput::text(large)) })
        },
    ))
}

#[tokio::test]
async fn namespaced_tool_returns_prefixed_name() -> Result<()> {
    let inner = mock_tool("run", "Run a command")?;
    let tool = NamespacedTool::new(inner, "test", "__", MAX_TOOL_OUTPUT_BYTES);

    assert_eq!(tool.name(), "test__run");
    Ok(())
}

#[tokio::test]
async fn namespaced_tool_returns_prefixed_description() -> Result<()> {
    let inner = mock_tool("exec", "Execute something")?;
    let tool = NamespacedTool::new(inner, "mcp", "__", MAX_TOOL_OUTPUT_BYTES);

    assert_eq!(tool.name(), "mcp__exec");
    assert_eq!(tool.description(), "Execute something");
    assert!(tool.parameters().is_object());
    Ok(())
}

#[tokio::test]
async fn namespaced_tool_call_delegates_to_inner() -> Result<()> {
    let inner = mock_tool("test", "Test tool")?;
    let tool = NamespacedTool::new(inner, "server", "__", MAX_TOOL_OUTPUT_BYTES);

    let result = tool.call(r#"{"arg": "value"}"#.to_string()).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "mock result");
    Ok(())
}

#[tokio::test]
async fn namespaced_tool_uses_custom_delimiter() -> Result<()> {
    let inner = mock_tool("info", "Get info")?;
    let tool = NamespacedTool::new(inner, "server", "::", MAX_TOOL_OUTPUT_BYTES);

    assert_eq!(tool.name(), "server::info");
    assert_eq!(tool.description(), "Get info");
    assert!(tool.parameters().is_object());
    Ok(())
}

#[tokio::test]
async fn namespaced_tool_truncates_large_output() -> Result<()> {
    let inner = large_mock_tool()?;
    let tool = NamespacedTool::new(inner, "server", "__", MAX_TOOL_OUTPUT_BYTES);

    let result = tool.call("{}".to_string()).await;
    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);

    let result_str = result.unwrap();
    assert!(
        result_str.contains("[output truncated:"),
        "NamespacedTool must truncate output exceeding MAX_TOOL_OUTPUT_BYTES; \
         got {} bytes with no marker",
        result_str.len()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// mcp_result_output tests
// ---------------------------------------------------------------------------

#[test]
fn mcp_result_output_text_block() {
    let result =
        rmcp::model::CallToolResult::success(vec![rmcp::model::ContentBlock::text("hello world")]);
    let output = super::mcp_result_output(&result).unwrap();
    assert_eq!(output, "hello world");
}

#[test]
fn mcp_result_output_image_block() {
    let result = rmcp::model::CallToolResult::success(vec![rmcp::model::ContentBlock::image(
        "base64data",
        "image/png",
    )]);
    let output = super::mcp_result_output(&result).unwrap();
    assert_eq!(output, "[image: image/png]");
}

#[test]
fn mcp_result_output_audio_block() {
    let result = rmcp::model::CallToolResult::success(vec![rmcp::model::ContentBlock::audio(
        "base64data",
        "audio/wav",
    )]);
    let output = super::mcp_result_output(&result).unwrap();
    assert_eq!(output, "[audio: audio/wav]");
}

#[test]
fn mcp_result_output_resource_block() {
    let result =
        rmcp::model::CallToolResult::success(vec![rmcp::model::ContentBlock::embedded_text(
            "file:///tmp/test.txt",
            "file content",
        )]);
    let output = super::mcp_result_output(&result).unwrap();
    assert_eq!(output, "[resource: file:///tmp/test.txt]");
}

#[test]
fn mcp_result_output_resource_link_block() {
    let resource = rmcp::model::Resource::new("https://example.com/resource", "example-resource");
    let result =
        rmcp::model::CallToolResult::success(vec![rmcp::model::ContentBlock::resource_link(
            resource,
        )]);
    let output = super::mcp_result_output(&result).unwrap();
    assert_eq!(output, "[resource_link: https://example.com/resource]");
}

#[test]
fn mcp_result_output_multiple_blocks() {
    let result = rmcp::model::CallToolResult::success(vec![
        rmcp::model::ContentBlock::text("first"),
        rmcp::model::ContentBlock::image("data", "image/jpeg"),
        rmcp::model::ContentBlock::text("last"),
    ]);
    let output = super::mcp_result_output(&result).unwrap();
    assert_eq!(output, "first\n[image: image/jpeg]\nlast");
}

#[test]
fn mcp_result_output_empty_content() {
    let result = rmcp::model::CallToolResult::success(vec![]);
    let output = super::mcp_result_output(&result).unwrap();
    assert_eq!(output, "");
}
