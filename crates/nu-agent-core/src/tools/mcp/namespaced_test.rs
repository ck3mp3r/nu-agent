use super::*;
use crate::tools::limits::MAX_TOOL_OUTPUT_BYTES;
use rig::tool::{ToolDyn, ToolError};
use rig::wasm_compat::WasmBoxedFuture;

/// Mock tool that implements ToolDyn for testing
struct MockTool {
    name: String,
    description: String,
}

impl MockTool {
    fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
        }
    }
}

impl ToolDyn for MockTool {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn description(&self) -> String {
        self.description.clone()
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({})
    }

    fn call<'a>(&'a self, _args: String) -> WasmBoxedFuture<'a, Result<String, ToolError>> {
        Box::pin(async { Ok("mock result".to_string()) })
    }
}

#[tokio::test]
async fn namespaced_tool_returns_prefixed_name() {
    // RED: Write failing test first
    let inner = MockTool::new("run", "Run a command");
    let tool = NamespacedTool::new(Box::new(inner), "nu", "__", MAX_TOOL_OUTPUT_BYTES);

    assert_eq!(tool.name(), "nu__run");
}

#[tokio::test]
async fn namespaced_tool_returns_prefixed_description() {
    // RED: Test that description and parameters delegate to inner tool
    let inner = MockTool::new("exec", "Execute something");
    let tool = NamespacedTool::new(Box::new(inner), "mcp", "__", MAX_TOOL_OUTPUT_BYTES);

    assert_eq!(tool.name(), "mcp__exec");
    assert_eq!(tool.description(), "Execute something");
    assert!(tool.parameters().is_object());
}

#[tokio::test]
async fn namespaced_tool_call_delegates_to_inner() {
    // RED: Test that call delegates to inner tool
    let inner = MockTool::new("test", "Test tool");
    let tool = NamespacedTool::new(Box::new(inner), "server", "__", MAX_TOOL_OUTPUT_BYTES);

    let result = tool.call(r#"{"arg": "value"}"#.to_string()).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "mock result");
}

#[tokio::test]
async fn namespaced_tool_uses_custom_delimiter() {
    // RED: Test custom delimiter
    let inner = MockTool::new("info", "Get info");
    let tool = NamespacedTool::new(Box::new(inner), "server", "::", MAX_TOOL_OUTPUT_BYTES);

    assert_eq!(tool.name(), "server::info");
    assert_eq!(tool.description(), "Get info");
    assert!(tool.parameters().is_object());
}

/// Mock tool that returns a large result exceeding MAX_TOOL_OUTPUT_BYTES.
struct LargeMockTool;

impl ToolDyn for LargeMockTool {
    fn name(&self) -> String {
        "big_tool".to_string()
    }

    fn description(&self) -> String {
        "Returns lots of data".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({})
    }

    fn call<'a>(&'a self, _args: String) -> WasmBoxedFuture<'a, Result<String, ToolError>> {
        let large = "x".repeat(MAX_TOOL_OUTPUT_BYTES + 1_000);
        Box::pin(async move { Ok(large) })
    }
}

#[tokio::test]
async fn namespaced_tool_truncates_large_output() {
    let inner = LargeMockTool;
    let tool = NamespacedTool::new(Box::new(inner), "server", "__", MAX_TOOL_OUTPUT_BYTES);

    let result = tool.call("{}".to_string()).await;
    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);

    let result_str = result.unwrap();
    assert!(
        result_str.contains("[output truncated:"),
        "NamespacedTool must truncate output exceeding MAX_TOOL_OUTPUT_BYTES; \
         got {} bytes with no marker",
        result_str.len()
    );
}
