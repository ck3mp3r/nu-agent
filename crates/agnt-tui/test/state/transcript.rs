//! Unit tests for the transcript store.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "transcript/cap.rs"]
mod cap;

#[path = "transcript/eviction.rs"]
mod eviction;

#[path = "transcript/height_index.rs"]
mod height_index;

#[path = "transcript/hydration.rs"]
mod hydration;

#[path = "transcript/markdown.rs"]
mod markdown;

#[path = "transcript/push_entry.rs"]
mod push_entry;

#[path = "transcript/shift_indices.rs"]
mod shift_indices;

#[path = "transcript/support.rs"]
mod support;

// The monolith resolved `super::<item>` against `crate::state`.
// Sub-files now resolve `super::` against this module root, so the shared
// fixtures are re-exported here.
use crate::state::hydrate_by_replay;
use support::*;

// endregion: --- Modules
