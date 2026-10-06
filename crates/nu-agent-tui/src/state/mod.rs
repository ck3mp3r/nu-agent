mod app_state;
pub(crate) mod code_block;
mod compaction;
mod input;
mod input_history;
mod lifecycle;
mod llm;
mod mcp;
mod permission;
mod picker;
mod prompt_queue;
mod scroll;
pub mod selection;
pub(crate) mod spacer;
mod status;
mod tool;
mod tool_calls;
pub mod tool_parsing;
mod transcript_store;
mod turn;

pub use app_state::*;
pub use compaction::*;
pub use input::*;
pub use llm::*;
pub use mcp::*;
pub use permission::*;
pub use picker::*;
pub use scroll::*;
pub use status::*;
pub use tool::*;
#[cfg(test)]
pub(crate) use tool_parsing::parse_persisted_tool_status_line;
pub use transcript_store::*;
pub use turn::*;

#[cfg(test)]
#[path = "../../test/state/code_block.rs"]
mod code_block_test;

#[cfg(test)]
#[path = "../../test/state/selection.rs"]
mod selection_test;

#[cfg(test)]
#[path = "../../test/state/transcript.rs"]
mod transcript_test;

#[cfg(test)]
#[path = "../../test/state/input.rs"]
mod input_test;

#[cfg(test)]
#[path = "../../test/state/scroll.rs"]
mod scroll_test;

#[cfg(test)]
#[path = "../../test/state/lifecycle.rs"]
mod lifecycle_test;

#[cfg(test)]
#[path = "../../test/state/permission.rs"]
mod permission_test;

#[cfg(test)]
#[path = "../../test/state/mcp.rs"]
mod mcp_test;

#[cfg(test)]
#[path = "../../test/state/picker.rs"]
mod picker_test;

#[cfg(test)]
#[path = "../../test/state/status.rs"]
mod status_test;

#[cfg(test)]
#[path = "../../test/state/tool.rs"]
mod tool_test;

#[cfg(test)]
#[path = "../../test/state/llm.rs"]
mod llm_test;

#[cfg(test)]
#[path = "../../test/state/compaction.rs"]
mod compaction_test;

#[cfg(test)]
#[path = "../../test/state/turn.rs"]
mod turn_test;

#[cfg(test)]
#[path = "../../test/state/test.rs"]
mod mod_test;
