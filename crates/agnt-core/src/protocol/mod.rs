pub mod agents;
pub mod contracts;
pub mod events;
pub mod mcp_management;
pub mod model_switching;
pub mod persona;
pub mod picker;
pub mod preamble;
pub mod session_management;
pub mod skills;
pub mod slash;

pub use events::{
    AutoResolutionAction, Key, RequestId, SystemEvent, SystemKind, ToolCall, ToolCallId,
    ToolOutcome, Usage,
};

#[cfg(test)]
#[path = "../../test/protocol/events.rs"]
mod events_test;

#[cfg(test)]
#[path = "../../test/protocol/session_management.rs"]
mod session_management_test;

#[cfg(test)]
#[path = "../../test/protocol/test.rs"]
mod test;
