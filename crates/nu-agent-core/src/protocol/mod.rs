pub mod agents;
pub mod cancellation;
pub mod compaction;
pub mod compaction_runtime;
pub mod contracts;
pub mod event;
pub mod mcp_management;
pub mod model_switching;
pub mod permission;
pub mod persona;
pub mod picker;
pub mod preamble;
pub mod prompt;
pub mod session_management;
pub mod skills;
pub mod slash;
pub mod tool_args;

#[cfg(test)]
mod agents_test;

#[cfg(test)]
mod compaction_test;

#[cfg(test)]
mod event_contract_test;

#[cfg(test)]
mod permission_test;

#[cfg(test)]
mod slash_test;

#[cfg(test)]
mod skills_test;
