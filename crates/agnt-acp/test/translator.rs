//! Unit tests for the session-record → ACP `SessionUpdate` translation layer.
//!
//! The suite is split into topical sub-files. Shared fixtures live in
//! [`support`]; every sub-file declares its own imports and `Result` alias.

// region:    --- Modules

#[path = "translator/tool_events/call_id.rs"]
mod call_id;
#[path = "translator/tool_events/completed.rs"]
mod completed;
#[path = "translator/drain_loop.rs"]
mod drain_loop;
#[path = "translator/event_mapping.rs"]
mod event_mapping;
#[path = "translator/permission_bridge.rs"]
mod permission_bridge;
#[path = "translator/tool_events/started.rs"]
mod started;
#[path = "translator/support.rs"]
mod support;

// endregion: --- Modules
