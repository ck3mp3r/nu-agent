use super::*;

// -- Test Support

pub(super) fn add_failing_tool<T: super::super::BuiltinTool>(
    toolset: &mut rig::tool::ToolSet,
    tool_name: &str,
) -> Result<(), EmptyToolName> {
    let tool_def = ToolDefinition {
        annotations: crate::types::ToolAnnotations::default(),
        name: ToolName::new(tool_name)?,
        description: "Always fails".to_string(),
        parameters: serde_json::json!({"type": "object", "properties": {}}),
    };
    toolset.add_dynamic_tool(super::super::make_dynamic_tool::<T>(
        tool_def,
        std::env::temp_dir(),
        100,
        crate::bus::Bus::default(),
    ));
    Ok(())
}
