//! Unit tests for the render loop's animation gating and event routing.
//!
//! The suite is split into topical sub-files. Each sub-file pulls the shared
//! imports in with `use super::*;`.

// region:    --- Modules

#[path = "render_animation/animation.rs"]
mod animation;

#[path = "render_animation/drain.rs"]
mod drain;

#[path = "render_animation/render_needed.rs"]
mod render_needed;

#[path = "render_animation/status_expiry.rs"]
mod status_expiry;

#[path = "render_animation/ui_events.rs"]
mod ui_events;

// endregion: --- Modules

// The monolith resolved `super::<item>` against `crate::runtime::test`.
// Sub-files now resolve `super::` against this module root, so the shared
// imports are re-exported here.
use super::*;
