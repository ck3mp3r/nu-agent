//! The abort/cancel battery: what Esc does to the abort state machine, one
//! topical file per behavior group.
//!
//! The dispatch suite's imports arrive through this module: every subfile pulls
//! them in with `use super::*;`, so the battery names its own record types and
//! nothing else.

// region:    --- Modules

use super::*;

#[path = "abort_cancel/arm_confirm.rs"]
mod arm_confirm;

#[path = "abort_cancel/expiry.rs"]
mod expiry;

#[path = "abort_cancel/idle_escape.rs"]
mod idle_escape;

#[path = "abort_cancel/settle.rs"]
mod settle;

#[path = "abort_cancel/surfaces.rs"]
mod surfaces;

// endregion: --- Modules
