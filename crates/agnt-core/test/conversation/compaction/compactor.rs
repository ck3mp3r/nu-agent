//! Tests for the `NuCompactor` rig `Compactor` adapter.
//!
//! `NuCompactor` (implemented in `compactor.rs`, created by the implementation
//! task) wraps the sliding-summary summarizer so it satisfies the rig 0.42.0
//! `Compactor` trait. These tests pin the observable behavior through the
//! public boundary: the produced `Artifact`, the summarizer LLM prompt input,
//! the bus events emitted during streaming, the optional `max_bytes` cap, and
//! the error mapping.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "compactor/artifact.rs"]
mod artifact;

#[path = "compactor/failures.rs"]
mod failures;

#[path = "compactor/formatting.rs"]
mod formatting;

#[path = "compactor/marker_store.rs"]
mod marker_store;

#[path = "compactor/model_swap.rs"]
mod model_swap;

#[path = "compactor/support.rs"]
mod support;

#[path = "compactor/truncation.rs"]
mod truncation;

// The monolith resolved `super::<helper>` against `crate::conversation::compaction`.
// Sub-files now resolve `super::` against this module root, so the shared
// fixtures are re-exported here.
use support::*;

// endregion: --- Modules
