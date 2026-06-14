use super::*;
use crate::types::ToolDefinition;
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

    fn definition<'a>(&'a self, _prompt: String) -> WasmBoxedFuture<'a, ToolDefinition> {
        let name = self.name.clone();
        let description = self.description.clone();
        Box::pin(async move {
            ToolDefinition {
                name,
                description,
                parameters: serde_json::json!({}),
            }
        })
    }

    fn call<'a>(&'a self, _args: String) -> WasmBoxedFuture<'a, Result<String, ToolError>> {
        Box::pin(async { Ok("mock result".to_string()) })
    }
}

#[tokio::test]
async fn namespaced_tool_returns_prefixed_name() {
    // RED: Write failing test first
    let inner = MockTool::new("run", "Run a command");
    let tool = NamespacedTool::new(Box::new(inner), "nu", "__");

    assert_eq!(tool.name(), "nu__run");
}

#[tokio::test]
async fn namespaced_tool_definition_has_prefixed_name() {
    // RED: Test that definition returns namespaced name
    let inner = MockTool::new("exec", "Execute something");
    let tool = NamespacedTool::new(Box::new(inner), "mcp", "__");

    let definition = tool.definition("test prompt".to_string()).await;
    assert_eq!(definition.name, "mcp__exec");
    assert_eq!(definition.description, "Execute something");
}

#[tokio::test]
async fn namespaced_tool_call_delegates_to_inner() {
    // RED: Test that call delegates to inner tool
    let inner = MockTool::new("test", "Test tool");
    let tool = NamespacedTool::new(Box::new(inner), "server", "__");

    let result = tool.call(r#"{"arg": "value"}"#.to_string()).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "mock result");
}

#[tokio::test]
async fn namespaced_tool_uses_custom_delimiter() {
    // RED: Test custom delimiter
    let inner = MockTool::new("info", "Get info");
    let tool = NamespacedTool::new(Box::new(inner), "server", "::");

    assert_eq!(tool.name(), "server::info");

    let definition = tool.definition("prompt".to_string()).await;
    assert_eq!(definition.name, "server::info");
}
