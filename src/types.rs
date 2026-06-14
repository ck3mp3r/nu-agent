//! Internal type aliases and re-exports for rig types.
//!
//! All internal modules import rig types from here rather than directly from
//! `rig::completion`. This creates a single seam for future rig version migrations.
//!
//! See: rig 0.39.0 migration note (c5t note 50a9d896) — Option<Usage> → Usage breaking change.

// Core conversation message types
pub(crate) use rig::completion::Message;
#[cfg(test)]
pub(crate) use rig::completion::message::ToolResult;
pub(crate) use rig::completion::message::{
    AssistantContent, Text, ToolCall, ToolFunction, ToolResultContent, UserContent,
};

// Tool definitions (used in runtime and turn execution)
pub(crate) use rig::completion::ToolDefinition;

// Memory type (used in compaction and turn execution)
pub(crate) use rig::memory::InMemoryConversationMemory;
