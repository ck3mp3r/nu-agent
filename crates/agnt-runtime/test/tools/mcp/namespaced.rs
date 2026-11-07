use super::*;
use crate::tools::limits::MAX_TOOL_OUTPUT_BYTES;
use crate::types::{EmptyToolName, ToolName};
use rig::tool::{DynamicTool, ToolExecutionError, ToolOutput};

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
async fn namespaced_tool_name_and_description_table() -> Result<()> {
    // -- Setup & Fixtures
    let cases: Vec<(&str, &str, &str, &str, &str)> = vec![
        ("default delimiter", "run", "Run a command", "test", "__"),
        (
            "prefixed description",
            "exec",
            "Execute something",
            "mcp",
            "__",
        ),
    ];

    // -- Exec & Check
    for (name, tool_name, description, server, delimiter) in cases {
        let inner = mock_tool(tool_name, description)?;
        let tool = NamespacedTool::new(inner, server, delimiter, MAX_TOOL_OUTPUT_BYTES);

        assert_eq!(
            tool.name(),
            format!("{server}{delimiter}{tool_name}"),
            "case '{name}': name"
        );
        assert_eq!(
            tool.description(),
            description,
            "case '{name}': description"
        );
        assert!(tool.parameters().is_object(), "case '{name}': parameters");
    }
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
fn mcp_result_output_table() {
    // -- Setup & Fixtures
    let cases: Vec<(&str, Vec<rmcp::model::ContentBlock>, &str)> = vec![
        (
            "text block",
            vec![rmcp::model::ContentBlock::text("hello world")],
            "hello world",
        ),
        (
            "image block",
            vec![rmcp::model::ContentBlock::image("base64data", "image/png")],
            "[image: image/png]",
        ),
        (
            "audio block",
            vec![rmcp::model::ContentBlock::audio("base64data", "audio/wav")],
            "[audio: audio/wav]",
        ),
        (
            "resource block",
            vec![rmcp::model::ContentBlock::embedded_text(
                "file:///tmp/test.txt",
                "file content",
            )],
            "[resource: file:///tmp/test.txt]",
        ),
        (
            "resource link block",
            vec![rmcp::model::ContentBlock::resource_link(
                rmcp::model::Resource::new("https://example.com/resource", "example-resource"),
            )],
            "[resource_link: https://example.com/resource]",
        ),
        (
            "multiple blocks",
            vec![
                rmcp::model::ContentBlock::text("first"),
                rmcp::model::ContentBlock::image("data", "image/jpeg"),
                rmcp::model::ContentBlock::text("last"),
            ],
            "first\n[image: image/jpeg]\nlast",
        ),
        ("empty content", vec![], ""),
    ];

    // -- Exec & Check
    for (name, content, expected) in cases {
        let result = rmcp::model::CallToolResult::success(content);
        let output = super::mcp_result_output(&result).unwrap();
        assert_eq!(output, expected, "case '{name}'");
    }
}

// ---------------------------------------------------------------------------
// NamespacedTool error propagation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn namespaced_tool_call_propagates_inner_error() -> Result<()> {
    // -- Setup & Fixtures
    let inner = DynamicTool::new(
        ToolName::new("failing")?,
        "Always fails",
        serde_json::json!({}),
        |_args| Box::pin(async { Err(ToolExecutionError::other("inner tool exploded")) }),
    );
    let tool = NamespacedTool::new(inner, "server", "__", MAX_TOOL_OUTPUT_BYTES);

    // -- Exec
    let result = tool.call("{}".to_string()).await;

    // -- Check
    let error = result.err().ok_or("an inner failure must fail the call")?;
    assert_eq!(error.kind(), rig::tool::ToolErrorKind::Other);
    assert!(
        error.message().contains("inner tool exploded"),
        "the inner error message must survive the wrapper, got: {}",
        error.message()
    );
    Ok(())
}
