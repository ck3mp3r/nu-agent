//! Unit tests for the message repair passes.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "repair/empty_results.rs"]
mod empty_results;

#[path = "repair/integrity_extras.rs"]
mod integrity_extras;

#[path = "repair/invalid_args.rs"]
mod invalid_args;

#[path = "repair/jsonl_heal.rs"]
mod jsonl_heal;

#[path = "repair/passes.rs"]
mod passes;

#[path = "repair/summary_extras.rs"]
mod summary_extras;

#[path = "repair/support.rs"]
mod support;

// The monolith resolved `super::<item>` against `crate::session`.
// Sub-files now resolve `super::` against this module root, so the shared
// fixtures are re-exported here.
use support::*;

// endregion: --- Modules

use super::repair::{
    fix_empty_tool_results, fix_invalid_tool_arguments, fix_tool_call_integrity,
    inject_assistant_after_dangling_tool_results, merge_consecutive_same_role,
    remove_empty_messages, repair_messages, trim_trailing_user,
};
