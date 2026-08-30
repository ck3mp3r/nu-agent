pub mod agents;
pub mod cancellation;
pub mod compaction;
pub mod contracts;
pub mod event;
pub mod mcp_management;
pub mod model_switching;
pub mod persona;
pub mod picker;
pub mod preamble;
pub mod prompt;
pub mod session_management;
pub mod skills;
pub mod slash;
pub mod tool_args;

#[cfg(test)]
mod session_management_test;

#[cfg(test)]
mod test;
