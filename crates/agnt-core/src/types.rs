//! Internal type aliases and re-exports for rig types.
//!
//! All internal modules import rig types from here rather than directly from
//! `rig::completion`. This creates a single seam for future rig version migrations.
//!
//! See: rig 0.39.0 migration note (c5t note 50a9d896) — Option<Usage> → Usage breaking change.

// Core conversation message types
pub use rig::completion::Message;
pub use rig::completion::message::ToolResult;
pub use rig::completion::message::{
    AssistantContent, AssistantMessage, CallId, EmptyToolName, Opaque, ProviderCallId, Text,
    ToolCall, ToolFunction, ToolName, ToolResultContent, UserContent,
};

// Tool definitions (used in runtime and turn execution)
pub use rig::completion::ToolDefinition;

// Runtime model types (used by the ACP and plugin runtimes)
pub use rig::DynModel;
pub use rig::operation::Completion;

// Tool server types (used to register and dispatch dynamic tools)
pub use rig::tool::server::{ToolServer, ToolServerHandle};
pub use rig::tool::{DynamicTool, ToolExecutionError, ToolOutput};

// Copilot provider config and OAuth device-code auth
pub use rig::providers::copilot::CopilotConfig;
pub use rig::providers::copilot::auth::*;

// Test double for provider models (used by downstream crate tests)
pub use rig::test_utils::MockCompletionModel;
