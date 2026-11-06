//! Level 2 aliases whose events belong to this crate.
//!
//! The Level 1 facade and the five lifecycle alias pairs live in
//! [`agnt_kernel::bus`]; the parent module re-exports them so a consumer of
//! this crate keeps one import path. The three channels below carry this
//! crate's own vocabulary, so they stay here until that vocabulary lifts.

use agnt_kernel::bus::{BroadcastRx, BroadcastTx};

use crate::orchestrator::UiStateEvent;
use crate::protocol::events::SystemEvent;

// region:    --- Level 2 Aliases

/// Compaction lifecycle channel.
///
/// Carries the compaction facts of the one `SystemEvent` vocabulary; the
/// channel stays separate from `ui_event` because only the orchestrator acts
/// on the request.
pub type CompactionTx = BroadcastTx<SystemEvent>;
/// Compaction lifecycle channel receiver.
pub type CompactionRx = BroadcastRx<SystemEvent>;
/// UI state channel.
pub type UiStateTx = BroadcastTx<UiStateEvent>;
/// UI state channel receiver.
pub type UiStateRx = BroadcastRx<UiStateEvent>;
/// Unified UI event channel.
pub type UiEventTx = BroadcastTx<SystemEvent>;
/// Unified UI event channel receiver.
pub type UiEventRx = BroadcastRx<SystemEvent>;

// endregion: --- Level 2 Aliases
