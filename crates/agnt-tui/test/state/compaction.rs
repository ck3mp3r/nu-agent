//! Compaction-domain reducer tests: compaction block rendering, streaming
//! summary projection, and completion/failure bookkeeping. Assertions moved
//! 1:1 from the former `interaction/reducer_test.rs` `reduce_ui_event_impl`
//! effect tests, driven through `CompactionState::reduce_compaction_event`.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "compaction/artifact.rs"]
mod artifact;

#[path = "compaction/markdown.rs"]
mod markdown;

#[path = "compaction/support.rs"]
mod support;

#[path = "compaction/transitions.rs"]
mod transitions;

// The monolith resolved `super::<item>` against `crate::state`.
// Sub-files now resolve `super::` against this module root, so the shared
// fixtures are re-exported here.
use support::*;

// endregion: --- Modules
