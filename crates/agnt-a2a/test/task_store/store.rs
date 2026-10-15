//! In-memory task store tests.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "store/concurrency.rs"]
mod concurrency;

#[path = "store/crud.rs"]
mod crud;

#[path = "store/idempotency.rs"]
mod idempotency;

#[path = "store/lifecycle.rs"]
mod lifecycle;

#[path = "store/list.rs"]
mod list;

#[path = "store/push_configs.rs"]
mod push_configs;

#[path = "store/push_payload.rs"]
mod push_payload;

#[path = "store/subscribe.rs"]
mod subscribe;

#[path = "store/support.rs"]
mod support;

#[path = "store/trait_contract.rs"]
mod trait_contract;

#[path = "store/transitions.rs"]
mod transitions;

// The monolith resolved `super::<helper>` against `crate::task_store`.
// Sub-files now resolve `super::` against this module root, so the shared
// fixtures are re-exported here.
use support::*;

// endregion: --- Modules
