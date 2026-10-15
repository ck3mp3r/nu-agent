//! Client function tests.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "functions/agent_card.rs"]
mod agent_card;

#[path = "functions/get_cancel.rs"]
mod get_cancel;

#[path = "functions/list.rs"]
mod list;

#[path = "functions/send.rs"]
mod send;

#[path = "functions/spec_url.rs"]
mod spec_url;

#[path = "functions/support.rs"]
mod support;

// The monolith resolved `super::<helper>` against `crate::client`.
// Sub-files now resolve `super::` against this module root, so the shared
// fixtures are re-exported here.
use super::shared::*;
use support::*;

// endregion: --- Modules
