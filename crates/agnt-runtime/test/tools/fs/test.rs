//! Filesystem tool test suite.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "test/core_ops.rs"]
mod core_ops;
#[path = "test/create.rs"]
mod create;
#[path = "test/diff.rs"]
mod diff;
#[path = "test/edit_validation.rs"]
mod edit_validation;
#[path = "test/mutation.rs"]
mod mutation;
#[path = "test/plan.rs"]
mod plan;
#[path = "test/read.rs"]
mod read;
#[path = "test/search_replace.rs"]
mod search_replace;
#[path = "test/support.rs"]
mod support;

// endregion: --- Modules

use super::core::{
    ConflictError, EditMatchMode, EditOccurrence, EditOperation, FileMetadata, MutateError,
    ReadRequest, apply_create_file, apply_full_content_mutation, apply_search_replace_edit,
    atomic_overwrite, check_conflict, metadata_for_path, plan_create_file,
    plan_create_from_content, plan_search_replace_from_content, read_file, version_token,
};
use super::diff::{DiffBounds, compute_edit_unified_diff_bounded};
use std::fs;
use std::path::Path;
use tempfile::tempdir;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// The monolith resolved `super::<helper>` against `crate::tools::fs`.
// Sub-files now resolve `super::` against this module root, so the shared
// fixtures are re-exported here.
use support::*;
