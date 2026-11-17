//! Dispatch-time skip for store-terminal external tasks.
//!
//! The suite is split into topical sub-files: the skip itself, the stale cancel
//! a skip leaves behind, and the fixtures both share. Each sub-file pulls the
//! shared fixtures in with `use super::*;`.

// region:    --- Modules

#[path = "cancel_dispatch_skip/stale_cancel.rs"]
mod stale_cancel;
#[path = "cancel_dispatch_skip/support.rs"]
mod support;
#[path = "cancel_dispatch_skip/terminal_skip.rs"]
mod terminal_skip;

// endregion: --- Modules

use super::*;

use support::*;
