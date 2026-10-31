mod adapter;
mod agent;
mod cancel;
mod get;
mod get_card;
mod list;
mod register;
mod registry;
mod resolve;
mod send;

#[cfg(test)]
#[path = "../../test/tools/defs.rs"]
mod defs_test;

#[cfg(all(test, feature = "integration"))]
#[path = "../../test/tools/test.rs"]
mod test;

pub use adapter::{A2aToolAdapter, A2aToolContext, A2aToolDef};
pub use registry::{Tool, ToolResult, a2a_tool_defs, register_tools_on_server};
