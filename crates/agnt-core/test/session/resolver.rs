//! Unit tests for the session resolver.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "resolver/conversion.rs"]
mod conversion;

#[path = "resolver/hydration.rs"]
mod hydration;

#[path = "resolver/rehydration.rs"]
mod rehydration;

#[path = "resolver/resolve_attach.rs"]
mod resolve_attach;

#[path = "resolver/support.rs"]
mod support;

#[path = "resolver/tool_result.rs"]
mod tool_result;

// The monolith resolved `super::<item>` against `crate::session`.
// Sub-files now resolve `super::` against this module root, so the shared
// fixtures are re-exported here.
use support::*;

// endregion: --- Modules

// `super::resolver::<fn>` paths in the moved tests resolve against this module
// root, so the production module is re-exported here.
use super::resolver;
