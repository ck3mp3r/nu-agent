//! Permission resolver test suite.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "permission_resolver/cancel.rs"]
mod cancel;

#[path = "permission_resolver/deny_reason.rs"]
mod deny_reason;

#[path = "permission_resolver/interactive_basic.rs"]
mod interactive_basic;

#[path = "permission_resolver/policy.rs"]
mod policy;

#[path = "permission_resolver/pre_authorize.rs"]
mod pre_authorize;

#[path = "permission_resolver/previewed_cwd.rs"]
mod previewed_cwd;

#[path = "permission_resolver/session_grants.rs"]
mod session_grants;

#[path = "permission_resolver/support.rs"]
mod support;

#[path = "permission_resolver/timeout.rs"]
mod timeout;

// endregion: --- Modules

// The monolith resolved `super::<item>` against `crate::hook::permission_resolver`.
// Sub-files now resolve `super::` against this module root, so the shared
// fixtures are re-exported here.
use support::*;

use super::*;
