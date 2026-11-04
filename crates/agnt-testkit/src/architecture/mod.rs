//! Architecture assertions: the structural properties the workspace is accepted against.
//!
//! Each assertion is a `pub` check under [`assertions`] that reads the workspace
//! and returns the violations it finds. The drivers are `#[cfg(test)]` modules in
//! `test/architecture/`, so a surface crate can call a check from its own test
//! tree without depending on this crate's drivers.
//!
//! The contract every check obeys:
//!
//! - Rust sources are parsed with `syn`, never line-scanned. `syn` is the only
//!   reader of Rust; a check that reads the lines of a `.rs` file is a failed
//!   check. A reported site is a span line, `Span::start().line`.
//! - Manifests are read as text. A `Cargo.toml` is not Rust source, so
//!   [`Manifest`] scans it and reports the line each key sits on.
//! - tree-sitter is closed as a path: no Rust grammar crate is vendored and no
//!   `tree-sitter` binary is run.
//! - No `cargo metadata` subprocess. The graph checks read the manifests.
//! - The ratchet is exact set equality, both directions, through
//!   [`assert_matches_baseline`]. A new violation fails the build, and so does a
//!   fixed violation whose baseline entry was not removed.

// region:    --- Modules

mod assertions;
mod report;
mod sources;

pub use assertions::*;
pub use report::*;
pub use sources::*;

// endregion: --- Modules

// region:    --- Review checks

/// The assertions this suite does not check mechanically, and the reason each
/// one is not mechanical yet.
///
/// An entry stays listed until its check exists. Dropping an entry hides a
/// property the workspace is still accepted against, so the list is the record
/// of what review carries.
///
/// Two of these wait on the same open decision. ARCH-2's term list needs an
/// agreed vocabulary, and the channel split needs the per-channel contract, so
/// neither can be written down yet: the four class names — `facts` included —
/// are placeholders and the contract that would settle them is owed. A term list
/// invented now would be wrong in the same way the names may be.
pub const REVIEW_CHECKS: &str = concat!(
    "ARCH-1  A change to how something is displayed stays inside `agnt-render` and the\n",
    "        surface that displays it.\n",
    "        Not mechanical: the measured object is the file set of a representative\n",
    "        change, which no reading of today's tree can observe.\n",
    "ARCH-2  `agnt-runtime` carries no rendering vocabulary and `agnt-render` carries no\n",
    "        orchestration vocabulary.\n",
    "        Blocked: the check needs a maintained term list, and the vocabulary decision\n",
    "        is pending. The four class names are placeholders and the per-channel\n",
    "        contract that would settle them is owed. The channel split (`ui_state`\n",
    "        carrying current values and operations) is blocked by the same decision.\n",
    "ARCH-3  One crate owns each extensible axis: a builtin tool touches `agnt-runtime`\n",
    "        and, only when it draws, `agnt-render`; a surface touches its own crate and\n",
    "        `agnt-cli`; an event variant touches `agnt-kernel` and the consumers that act\n",
    "        on it.\n",
    "        Not mechanical: measured by the file set of the change, asserted in review\n",
    "        against the axis table.\n",
    "ARCH-5  The end-to-end fabric runs the same code path with mocks and with real\n",
    "        adapters.\n",
    "        Not mechanical: it is what the E2E suite demonstrates. A mock path production\n",
    "        cannot take is a property of the wiring, not of the sources.\n",
    "ARCH-6  Every `pub` item has a production consumer outside its own crate, or is a\n",
    "        documented entry point with a named caller.\n",
    "        Not mechanical: \"documented entry point with a named caller\" is a judgement\n",
    "        per item, not a reading of the tree. The census half is not written yet.\n",
    "ARCH-7  No consumer is required to satisfy a capability it never calls.\n",
    "        Not mechanical: the check is \"for each method a fake implements, at least one\n",
    "        test reaches it\", which needs test-to-method reachability that no parser\n",
    "        reports.\n",
    "ARCH-8  (coverage) No function outside the composition root takes the whole\n",
    "        bus. The assertion pins reference hand-ins — `&Bus`, `&'a Bus` and a\n",
    "        qualified reference — and nothing else.\n",
    "        Pending: by-value parameters and returns, `Arc`/`Box` wraps and owned\n",
    "        fields are unmeasured today. The live shapes are\n",
    "        `agnt-core/src/orchestrator/config.rs:98` and `:33`, and the root field at\n",
    "        `agnt-core/src/conversation/runtime.rs:149`, which is root-legal. The\n",
    "        scoping ruling that extends the check to them rides the loop-owner slice;\n",
    "        the thirty-nine reference sites stay ratchet-locked meanwhile.\n",
    "ARCH-9  Only the composition root constructs infrastructure: HTTP clients, database\n",
    "        handles, TLS providers, subsystem tasks.\n",
    "        Not mechanical: the construction scan needs the root named per infrastructure\n",
    "        family, and that set is still moving.\n",
    "ARCH-10 No consumer names the aggregate runtime type; the aggregate exists only where\n",
    "        it is assembled and in the production alias.\n",
    "        Not mechanical: a reference count per crate is a direction, not an invariant —\n",
    "        the aggregate is still being assembled.\n",
    "ARCH-11 (enum-variant-field half) No field on a kernel record is a rendering, and a\n",
    "        field typed as an enum variant carries no display text.\n",
    "        Not mechanical: the classification is the consumer test in\n",
    "        `docs/design-choices.md` §2 — a consumer that finds may store, one that shows\n",
    "        may not — which is a judgement about how a field is used, not a property of\n",
    "        its syntax. The identity half is checked mechanically.\n",
);

// endregion: --- Review checks
