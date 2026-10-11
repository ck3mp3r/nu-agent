//! Unit tests for the ACP permission bridge.
//!
//! The suite is split into topical sub-files. Shared fixtures live in
//! [`support`]; every sub-file declares its own imports and `Result` alias.

// region:    --- Modules

#[path = "permission/content.rs"]
mod content;
#[path = "permission/decision_mapping.rs"]
mod decision_mapping;
#[path = "permission/deny_set.rs"]
mod deny_set;
#[path = "permission/pending.rs"]
mod pending;
#[path = "permission/request_shape.rs"]
mod request_shape;
#[path = "permission/support.rs"]
mod support;

// endregion: --- Modules
