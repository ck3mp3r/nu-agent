pub mod agents;
pub mod compaction;
pub mod contracts;
pub mod event;
pub mod events;
pub mod mcp_management;
pub mod model_switching;
pub mod persona;
pub mod picker;
pub mod preamble;
pub mod session_management;
pub mod skills;
pub mod slash;
pub mod tool_args;

pub use events::{
    AutoResolutionAction, Key, RequestId, SystemEvent, SystemKind, ToolCall, ToolCallId,
    ToolOutcome, Usage,
};
pub use tool_args::CallLine;

#[cfg(test)]
#[path = "../../test/protocol/events.rs"]
mod events_test;

#[cfg(test)]
#[path = "../../test/protocol/session_management.rs"]
mod session_management_test;

#[cfg(test)]
#[path = "../../test/protocol/test.rs"]
mod test;

#[cfg(test)]
#[path = "../../test/protocol/tool_args.rs"]
mod tool_args_test;
