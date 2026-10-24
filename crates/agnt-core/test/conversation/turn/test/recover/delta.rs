//! `persist_turn_delta` tests: the anchored slice, the records merge and the
//! streamed-prose remainder.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "delta/anchoring.rs"]
mod anchoring;
#[path = "delta/empty_delta.rs"]
mod empty_delta;
#[path = "delta/prose.rs"]
mod prose;
#[path = "delta/records_merge.rs"]
mod records_merge;
#[path = "delta/skip_warning.rs"]
mod skip_warning;
#[path = "delta/stored_results.rs"]
mod stored_results;
#[path = "delta/support.rs"]
mod support;

use support::*;

// endregion: --- Modules

use super::*;

use crate::conversation::turn::executor::TurnRecovery;
use crate::hook::TurnToolRecord;
use crate::protocol::event::UiEvent;
