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
pub const REVIEW_CHECKS: &str = concat!(
    "ARCH-1  A change to how something is displayed stays inside `agnt-render` and the\n",
    "        surface that displays it.\n",
    "        Not mechanical: the measured object is the file set of a representative\n",
    "        change, which no reading of today's tree can observe.\n",
    "ARCH-2  `agnt-runtime` carries no rendering vocabulary and `agnt-render` carries no\n",
    "        orchestration vocabulary.\n",
    "        Pinned: a maintained term list — the six bus concerns, their six payload\n",
    "        families and the `Bus` type they are reached through, read off\n",
    "        `docs/bus-channels.md`, on the drawer's side, and the drawer's published\n",
    "        vocabulary on the runtime's side — is matched against the measured public\n",
    "        surface: `pub` item names and `pub use` leaves; `pub` free-function\n",
    "        signatures; the field types of a `pub` struct and the variant field types of\n",
    "        a `pub` enum; the method signatures of a `pub` trait; and the `pub` method\n",
    "        signatures of an inherent impl whose self type is a `pub` type declared in\n",
    "        the same file. The ratchet records what the scan finds today: nothing — the\n",
    "        field, trait-method and inherent-method shapes measured zero sites when they\n",
    "        landed. Limits, bound for bound: (1) a name a macro expands to is not in the\n",
    "        parsed tree and is unmeasured; (2) an inherent impl is resolved\n",
    "        file-locally, so an `impl` in another file from its `pub` type is not\n",
    "        scanned; (3) matching is exact, and three shared English words (`Theme`,\n",
    "        `Markdown`, `Display`) stay off the list by design, so a compound built from\n",
    "        an omitted word (`MarkdownSection`) is not a term. The scan reads parsed\n",
    "        symbols, so a word in a doc comment or a string literal is not a site.\n",
    "ARCH-3  One crate owns each extensible axis: a builtin tool touches `agnt-runtime`\n",
    "        and, only when it draws, `agnt-render`; a surface touches its own crate and\n",
    "        `agnt-cli`; an event variant touches `agnt-kernel` and the consumers that act\n",
    "        on it.\n",
    "        Not mechanical: measured by the file set of the change, asserted in review\n",
    "        against the axis table.\n",
    "ARCH-5  The end-to-end fabric runs the same code path with mocks and with real\n",
    "        adapters, and no test double reaches production source.\n",
    "        Pinned, source half: every `rig::test_utils` reference a production source\n",
    "        names is ratcheted, and the row holds the one sanctioned LLM-wire mock,\n",
    "        `CachedProviderClient::Mock`. A use tree and a type path are both read.\n",
    "        The wiring half stays E2E: it is what the suite demonstrates, and a mock\n",
    "        path production cannot take is a property of the wiring, not of the\n",
    "        sources.\n",
    "        Blind spots, recorded: a test-utils module imported as a group member\n",
    "        (`use rig::{providers, test_utils};` then `test_utils::...`), a reference\n",
    "        written through a renamed crate (`use rig as r;` then `r::test_utils::...`),\n",
    "        and a reference a macro body expands to are unmeasured — the census reads\n",
    "        the parsed use tree and path expressions, never an expansion.\n",
    "ARCH-6  Every `pub` item has a production consumer outside its own crate, or is a\n",
    "        documented entry point with a named caller.\n",
    "        Pinned, census half: the surface ratcheted per crate under\n",
    "        `test/architecture/baselines_arch6/` is both\n",
    "        (a) every DECLARED `pub` item — a declaration site; `pub use` leaves are\n",
    "        excluded — and\n",
    "        (b) every `pub` member of an inherent impl whose self type the crate\n",
    "        surfaces: declares `pub` in a public module chain, or re-exports through a\n",
    "        `pub use` leaf. A trait implementation's members carry no visibility of\n",
    "        their own and are outside the census.\n",
    "        Each half is an exact set in its own file — `ROWS` the declared items,\n",
    "        `MEMBER_ROWS` the impl members — and the driver compares both per crate,\n",
    "        asserts the per-crate partition is total, and fails on any crate the walk\n",
    "        reports on that the record does not name. A fresh unconsumed item or\n",
    "        member fails in the crate file that owes it.\n",
    "        Status is a RECORD-THEN-DRAIN BACKLOG, not compliance. The basis is the\n",
    "        measured record: 716 declared items and 557 impl members — 1273 sites —\n",
    "        are surface awaiting a drain ruling. The checker bites on NEW surface only,\n",
    "        and the baseline only ever shrinks: a drained item's row leaves in the\n",
    "        same commit as its declaration. The judgement half — \"documented entry\n",
    "        point with a named caller\" — stays review and returns with the drain\n",
    "        ruling.\n",
    "        Two limits, bound for bound:\n",
    "        (1) reachability is not computed across files. A site counts when its own\n",
    "        visibility is `pub` and the inline module chain around it is public, so a\n",
    "        `pub` item in a private submodule file is recorded too. The member walk\n",
    "        resolves an `impl` crate-wide by self-type name, so the type's\n",
    "        declaration file is not consulted.\n",
    "        (2) consumer evidence is a path segment or a method name:\n",
    "        a consumer's module segment that equals an item's name suppresses\n",
    "        that item's row, whether or not the path reaches the item, and a\n",
    "        receiver call `<expr>.<name>(...)` in ANOTHER crate suppresses `<name>`\n",
    "        with no receiver-type attribution. The declaring crate's own calls are\n",
    "        not evidence: a crate calling its own method is the same crate the row\n",
    "        is about. Name-level matching over-counts consumers across the crates it\n",
    "        does not own; that direction is deliberate, and it retired 340 of the\n",
    "        1613 rows recorded before the method evidence landed (impl members 869\n",
    "        → 557).\n",
    "        A RE-MEASURE of the census's own surface is the one exception to \"only\n",
    "        ever shrinks\": when the walk's definition changes — this entry's landing\n",
    "        added the impl-member half — the re-recorded rows may grow, and the commit\n",
    "        that changes the walk states the new measured totals. A re-measure is not\n",
    "        a drain and inherits no drain credit.\n",
    "        DRAIN-VERIFICATION BAR: every row removal proves by AST (`ast_refs`/\n",
    "        `ast_query`) that the consumer-absence claim held at removal time and that\n",
    "        the declaration is deleted; a removal without that evidence is not a drain.\n",
    "ARCH-7  No consumer is required to satisfy a capability it never calls.\n",
    "        Not mechanical: the check is \"for each method a fake implements, at least one\n",
    "        test reaches it\", which needs test-to-method reachability that no parser\n",
    "        reports.\n",
    "ARCH-8  No function outside the composition root takes or hands out the whole\n",
    "        bus. Covered: `&Bus` in any spelling, an owned `Bus` parameter or\n",
    "        return, and an `Arc<Bus>`/`Box<Bus>` wrapper — parameter, return or\n",
    "        field.\n",
    "        Measured — seventeen sites ratcheted, the D4 drift among them\n",
    "        (`agnt-tui/src/interactive.rs:190`, the render loop's by-value `Bus`).\n",
    "        The rest are owned shapes: `orchestrator/config.rs:34` (builder field),\n",
    "        `:125` (`with_bus`) and `:211` (parts view field), the hook chain's two\n",
    "        fields, the compactor's field and two parameters, the turn executor's\n",
    "        field and parameter, the proxy's field, `builder.rs:537`,\n",
    "        `runtime_build.rs:60`, `executor/core.rs:108`, the acp session's field,\n",
    "        and the owned getter on the contract at `conversation/contracts.rs:27`.\n",
    "        Exempt: `conversation/runtime.rs:138` and\n",
    "        `conversation/runtime_parts.rs:28` — where the bus is assembled.\n",
    "ARCH-9  Only the composition root constructs infrastructure: HTTP clients, database\n",
    "        handles, TLS providers, subsystem tasks.\n",
    "        Measured (construction half, ARCH-9.b): `Client::builder`/`Client::new`,\n",
    "        `ensure_crypto_provider` calls, and the receiver-gated install — a\n",
    "        `…::default_provider().install_default()` counts only when it is exercised\n",
    "        as that install act, so a method of the name on another receiver never\n",
    "        fires — are ratcheted by the recorded set of ten sites. Seven\n",
    "        `agnt-runtime` rows: six are the composition-root stream's to drain (the\n",
    "        shared-client factory), and one is the installer's own body\n",
    "        (`utils/crypto.rs`), recorded so the wrapper and its callers stay one act\n",
    "        each. Two `agnt-cli` rows: the injected A2A transport's client construction\n",
    "        (`dispatch/a2a.rs`) and the binary's own install in `main.rs`, its own\n",
    "        composition root per the ADR/6 ruling. One `agnt-plugin` row: the plugin\n",
    "        binary's `main.rs`, the same ruling — the process-wide provider must be\n",
    "        installed where its process starts. `agnt-a2a` declares none: the mDNS\n",
    "        browse thread fetches through the transport its composition root hands\n",
    "        down, entering the owner's runtime through a handed-down handle, so the\n",
    "        installer body, its call, and the client are gone. The check\n",
    "        reads parsed expressions: a definition and a doc comment never fire. Blind\n",
    "        spot: `syn`'s visit walks parsed tokens, not a macro's expansion, so a\n",
    "        client built inside a macro body is unmeasured — no such construction\n",
    "        exists today.\n",
    "ARCH-10 No consumer names the aggregate runtime type; the aggregate exists only where\n",
    "        it is assembled and in the production alias.\n",
    "        Measured — the two names (`AgentRuntime`, `AgentConversationRuntime`)\n",
    "        locked to the nine recorded acp sites; three doc lines structurally invisible;\n",
    "        the root files exempt; the CLI drained (`52ff5251` → `37a519a3`); blind spots\n",
    "        unchanged (alias-laundering, macro-generated references, glob imports). The\n",
    "        compose call at acp/runtime.rs:412 is the invisible path that made the\n",
    "        headline 'nineteen' a hand-count.\n",
    "ARCH-11 (enum-variant-field half) No field on a kernel record is a rendering, and a\n",
    "        field typed as an enum variant carries no display text.\n",
    "        Not mechanical: the classification is the consumer test in\n",
    "        `docs/design-choices.md` §2 — a consumer that finds may store, one that shows\n",
    "        may not — which is a judgement about how a field is used, not a property of\n",
    "        its syntax. The identity half is checked mechanically.\n",
);

// endregion: --- Review checks
