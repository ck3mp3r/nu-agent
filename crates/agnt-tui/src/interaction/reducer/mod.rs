//! The surface's fold: every bus datum and every surface input, reduced into
//! [`AppState`](crate::state::AppState).
//!
//! The fold is split by concern: `fold` carries the contract and the entry
//! point, `records` the record and control vocabulary, `keys` the key lane and
//! its keymaps, `submit` the submit and picker-accept lanes, and `support` the
//! actions the lanes call.

// region:    --- Modules

mod fold;
mod keys;
mod records;
mod submit;
mod support;

pub use fold::{Fold, RenderInput, reduce};

pub(crate) use records::prompt_facts;

pub(crate) use support::VISUAL_REQUIRES_TRANSCRIPT_FOCUS_STATUS;

// The status strings and the permission-card helper are consumed by the test
// suites at this module path; the library build reaches them where they live.
#[cfg(test)]
pub(crate) use records::apply_permission_request_display;

#[cfg(test)]
pub(crate) use support::{ESC_ABORT_CONFIRM_STATUS, ESC_SETTLED_STATUS};

// endregion: --- Modules
