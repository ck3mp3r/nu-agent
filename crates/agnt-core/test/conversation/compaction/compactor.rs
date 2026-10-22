//! Tests for `NuCompactor`'s rig-memory compaction port.
//!
//! The compactor loads the conversation from the rig store, clears old tool
//! outputs, plans and streams the summary, splices it in through
//! `rig_memory::apply_summary`, and saves. These tests pin that flow through the
//! public boundary: the persisted conversation, the `rig.compaction` author, the
//! bus events, and the error mapping.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "compactor/clearing.rs"]
mod clearing;

#[path = "compactor/failures.rs"]
mod failures;

#[path = "compactor/model_swap.rs"]
mod model_swap;

#[path = "compactor/splice.rs"]
mod splice;

#[path = "compactor/support.rs"]
mod support;

use support::*;

// endregion: --- Modules
