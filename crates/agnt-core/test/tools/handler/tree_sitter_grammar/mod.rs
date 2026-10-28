//! Grammar-backed AST tool tests.
//!
//! The production grammar cache is a process-global `OnceLock`, so these tests
//! install a private tree-sitter config, a private parser directory, and a
//! private OS grammar cache holding a copy of the host's compiled `rust`
//! grammar. The whole module is `#[serial]` because it mutates process
//! environment variables, which `cargo test` shares across one process. Under
//! nextest every test runs in its own process, so `#[serial]` has no
//! cross-process effect there and only costs ordering within a process.
//!
//! When the host has no compiled `rust` grammar the tests return early: the
//! production code path cannot be reached without one.
//!
//! Measured cost model (2026-10-28, note 6cd3e7aa; evidence stack 5af01b91):
//! fixture install 1-5 ms, first tool execute 55-150 ms (the first use of the
//! copied grammar pays for the `dlopen`), and a whole test 0.079-0.291 s solo
//! and 0.016-0.108 s under parallel nextest. These bands are
//! environment-dependent measurements, not runtime guarantees: the syspolicyd
//! state described below moves them by orders of magnitude.
//!
//! The recorded flake (task 7129c457: 26.7-34.9 s solo, one pass at 17.1 s and
//! six 20 s timeouts in a group) is environmental. Each test copies the host
//! `rust.dylib` (ad-hoc linker-signed, rejected by `spctl -a -t execute`) to a
//! fresh per-test path, and macOS assesses every fresh copy through syspolicyd:
//! a Gatekeeper scan, provenance creation, and XProtect analysis per copy.
//! syspolicyd is a system-wide singleton, so a degraded daemon raises the
//! per-file cost to seconds and serializes the otherwise parallel tests. A
//! reboot resets that state; CPU load does not reproduce it.
//!
//! The obvious alternatives are rejected for recorded reasons. A symlink or
//! hardlink to the host grammar inode is vetoed on principle, because the test
//! must exercise the copy the user really gets; a hardlink is also impossible
//! here, since the host cache and the tempdir tree can sit on different APFS
//! volumes. A single shared fixture directory would break the per-test private
//! directory the fixture exists to provide. Raising the 20 s slow-timeout only
//! hides the mechanism and is forbidden.
//!
//! The suite is split into topical sub-files. Shared imports and fixtures live
//! here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

mod ast_nodes;
mod ast_query;
mod ast_refs;
mod ast_tree;
mod library_lookup;
mod support;
mod validation;

// endregion: --- Modules

use super::*;
use crate::bus::Bus;
use crate::tools::handler::ToolErrorKind;

use support::*;
