//! The shared rig type seam.
//!
//! Every module imports rig types from here rather than from `rig::completion`
//! directly, so a rig version migration has exactly one place to touch.

// region:    --- Modules

// endregion: --- Modules

// region:    --- Types

// Conversation message types
pub use rig::completion::Message;
pub use rig::completion::message::ToolResult;
pub use rig::completion::message::{
    AssistantContent, AssistantMessage, CallId, Document, DocumentData, DocumentSourceKind,
    EmptyToolName, Image, ImageDetail, MimeType, Opaque, ProviderCallId, Text, ToolCall,
    ToolFunction, ToolName, ToolResultContent, UserContent,
};

// Tool definitions (used in runtime and turn execution)
pub use rig::completion::{ToolAnnotations, ToolDefinition};

// Runtime model types (used by the ACP and plugin runtimes)
pub use rig::DynModel;
pub use rig::operation::Completion;

// Tool server types (used to register and dispatch dynamic tools)
pub use rig::tool::ToolErrorKind as RigToolErrorKind;
pub use rig::tool::ToolResult as ToolExecutionResult;
pub use rig::tool::server::{ToolServer, ToolServerHandle};
pub use rig::tool::{DynamicTool, ToolContext, ToolExecutionError, ToolOutput, ToolSet};

// Copilot provider OAuth device-code auth
pub use rig::providers::copilot::auth::{
    AuthError, AuthSource, Authenticator, DeviceCodeHandler, FileTokenStore,
};

// endregion: --- Types
