//! ARCH-6 (census half): every declared public item has a consumer outside its
//! crate.
//!
//! The judgement half — "or is a documented entry point with a named caller" —
//! stays a review row. This half is the part a parser can decide: an item a
//! crate declares `pub` and no other crate's `src` reaches is surface nothing
//! consumes.
//!
//! An item is one declaration site. A `pub use` leaf is not a site: it aliases a
//! name the crate does not own, so the census reads the items the crate defines.
//!
//! Evidence is a cross-crate path. A consumer names the crate it reaches
//! (`agnt_storage::…`) and then the item, in a type position, an expression or a
//! use tree, so the census reads paths and never identifiers on their own: a
//! field named `transcript` in another crate is not a consumer of
//! `agnt_storage`'s `transcript()`. Same-crate use is not evidence either: the
//! property is about surface across a crate boundary.
//!
//! A method reached through a receiver is evidence too, on its name alone: a
//! call site `<expr>.<name>(...)` records `<name>`, and an item another crate
//! calls that way is consumed. The receiver's type is not attributed, so a
//! method call in one crate suppresses a row of that name in every crate it does
//! not itself declare — the over-count is deliberate, and the cost is recorded
//! in the ARCH-6 review entry.
//!
//! An associated function called through an imported name is evidence the same
//! way. `use agnt_kernel::InferenceId;` binds the name, so a later
//! `InferenceId::mint()` carries no crate segment and is no receiver call: the
//! walk resolves each file's `use` trees to the crates they name, and attributes
//! that call to the declaring crate. A name the file never imported attributes
//! nothing — a local type's `Thing::mint()` is not evidence of another crate's
//! `mint`. The suppression is by name, like the receiver half, so it costs the
//! same over-count the review entry records.
//!
//! Blind spots, recorded rather than closed: a glob import
//! (`use agnt_runtime::types::*`) names no item, a macro-generated reference is
//! invisible to `syn`'s parsed tokens, and a crate renamed in the import
//! (`use agnt_kernel as k; k::…`) hides its paths behind the alias.
//!
//! One limit the record inherits, stated rather than hidden:
//!
//! - Reachability is not computed across files. An item is counted when its own
//!   visibility is `pub` and the inline module chain around it is public, so a
//!   `pub` item in a private submodule file is counted too — the census records
//!   declarations, which is what the assertion names. The member walk resolves an
//!   `impl` crate-wide, by self-type name against the crate's surfaced types, so
//!   the type's declaration file is not consulted.
//!
//! Three rules the walk applies, stated so a reader can check them against a row:
//!
//! - Associated functions are surface too. Two walks make one census: the item
//!   walk reads declarations, and the member walk
//!   ([`arch_6_impl_member_census`]) reads the `pub` members of inherent impls
//!   whose self type the crate surfaces — declares `pub` in a public module
//!   chain, or re-exports through a `pub use` leaf. A trait implementation's
//!   members carry no visibility of their own and are therefore outside both.
//! - Consumer evidence is a path segment. A consumer's module segment that
//!   happens to equal an item's name suppresses that item's row: a path into
//!   `agnt_storage::transcript` consumes a row named `transcript` whether or not
//!   the item is what the path reaches. Measured when the method evidence
//!   landed: 65 of 934 members (7.0%) were suppressed this way.
//! - An associated call is attributed by the crate its leading name was imported
//!   from. `InferenceId::mint()` after `use agnt_kernel::InferenceId;` consumes
//!   `mint` in `agnt_kernel`; a leading name the file did not import attributes
//!   nothing, so a local type of the same name consumes no row.
//!
//! A crate that appears in no crate's `[dependencies]` table and in some crate's
//! `[dev-dependencies]` table is dev-only: its surface exists for test trees, so
//! the production-consumer rule does not apply to it.
//!
//! Record-then-drain: the recorded rows are a drain backlog, not compliance. The
//! ratchet bites on new surface only, and it only ever shrinks — a removed item's
//! row leaves the baseline in the same commit.

// region:    --- Modules

mod census;
mod evidence;
mod imports;
mod paths;
mod support;

use crate::architecture::report::Violation;
use crate::architecture::sources::Workspace;

use census::{member_surface, surface};
use support::report;

// endregion: --- Modules

// region:    --- Functions

/// WHEN a crate declares a `pub` item and no other crate's `src` reaches it,
/// THEN the item SHALL have no cross-crate consumer and the assertion SHALL
/// report its declaration site, unless a baseline row records it.
pub fn arch_6_public_surface_consumer_census(ws: &Workspace) -> Vec<Violation> {
    report(ws, surface(ws))
}

/// WHEN a `pub` impl member on a census-visible type has no cross-crate
/// consumer, THEN the member SHALL be reported at its declaration site.
///
/// The member is the surface here, not the type: a type another crate reaches
/// does not consume its methods, so the member name is what must appear in a
/// consumer's path. A trait implementation's members carry no visibility of
/// their own and are therefore outside the walk; an inherent `impl`'s `pub`
/// members are the record's subject.
pub fn arch_6_impl_member_census(ws: &Workspace) -> Vec<Violation> {
    report(ws, member_surface(ws))
}

// endregion: --- Functions
