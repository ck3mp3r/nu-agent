//! Unit tests for the `SystemEvent` → ACP `SessionUpdate` translation layer.
//!
//! The suite is split into topical sub-files. Shared fixtures live in
//! [`support`]; every sub-file declares its own imports and `Result` alias.

// region:    --- Modules

#[path = "translator/drain_loop.rs"]
mod drain_loop;
#[path = "translator/event_mapping.rs"]
mod event_mapping;
#[path = "translator/permission_bridge.rs"]
mod permission_bridge;
#[path = "translator/support.rs"]
mod support;
#[path = "translator/tool_events.rs"]
mod tool_events;

// endregion: --- Modules
