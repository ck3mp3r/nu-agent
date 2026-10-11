// Re-export the tool definition functions that moved to core, so existing
// callers in the binary (assemble_tool_definitions, tests) don't break.
pub(crate) use agnt_core::conversation::builder::{ToolAssembly, assemble_tool_definitions};
