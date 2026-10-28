//! Grammar-backed AST tool tests.
//!
//! The production grammar cache is a process-global `OnceLock`, so these tests
//! install a private tree-sitter config, a private parser directory, and a
//! private OS grammar cache holding a copy of the host's compiled `rust`
//! grammar. The whole module is `#[serial]` because it mutates process
//! environment variables.
//!
//! When the host has no compiled `rust` grammar the tests return early: the
//! production code path cannot be reached without one.
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
