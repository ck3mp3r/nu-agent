//! The consumer evidence: cross-crate paths, receiver calls, imported-path
//! calls, and the dev-only crate test.

// region:    --- Modules

use std::collections::{BTreeMap, BTreeSet};

use syn::visit::Visit;

use super::imports::imported_crates;
use super::paths::{MethodCalls, Paths};

use crate::architecture::sources::Workspace;

// endregion: --- Modules

// region:    --- Functions

/// Every cross-crate path the workspace's sources write, keyed by (consumer
/// crate, consumed crate).
pub(super) fn cross_crate_references(
    ws: &Workspace,
) -> BTreeMap<(String, String), BTreeSet<String>> {
    let crates: BTreeSet<String> = crate_dirs(ws)
        .iter()
        .map(|krate| krate.replace('-', "_"))
        .collect();
    let mut references: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    for (label, _) in ws.sources() {
        let Some(file) = ws.rust_file(label) else {
            continue;
        };
        let Some((krate, _)) = crate_of(label) else {
            continue;
        };
        let imports = imported_crates(&crates, &file);
        let mut paths = Paths {
            crates: &crates,
            imports: &imports,
            found: BTreeMap::new(),
        };
        paths.visit_file(&file);
        for (target, names) in paths.found {
            references
                .entry((krate.to_string(), target))
                .or_default()
                .extend(names);
        }
    }
    references
}

/// The names `krate` publishes that some other crate reaches.
pub(super) fn consumed_names(
    references: &BTreeMap<(String, String), BTreeSet<String>>,
    krate: &str,
) -> BTreeSet<String> {
    let mut consumed = BTreeSet::new();
    for ((consumer, target), names) in references {
        if target == krate && consumer != krate {
            consumed.extend(names.iter().cloned());
        }
    }
    consumed
}

/// Every method-call name the workspace's sources write, keyed by the calling
/// crate's underscored name.
///
/// The key is normalized the same way [`called_names`] excludes the declaring
/// crate: a crate's own receiver calls (`self.take_pending_events()`) are not
/// cross-crate evidence, and keying by the crate directory's dashed name would
/// make every one of them look like an outside consumer.
pub(super) fn cross_crate_method_names(ws: &Workspace) -> BTreeMap<String, BTreeSet<String>> {
    let mut names: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (label, _) in ws.sources() {
        let Some(file) = ws.rust_file(label) else {
            continue;
        };
        let Some((krate, _)) = crate_of(label) else {
            continue;
        };
        let mut calls = MethodCalls::default();
        calls.visit_file(&file);
        if !calls.found.is_empty() {
            names
                .entry(krate.replace('-', "_"))
                .or_default()
                .extend(calls.found);
        }
    }
    names
}

/// The names `krate`'s methods are called by, from every crate but `krate`.
pub(super) fn called_names(
    names: &BTreeMap<String, BTreeSet<String>>,
    krate: &str,
) -> BTreeSet<String> {
    let mut called = BTreeSet::new();
    for (caller, methods) in names {
        if caller != krate {
            called.extend(methods.iter().cloned());
        }
    }
    called
}

/// Whether `krate` is dev-only: named in no `[dependencies]` table and in at
/// least one `[dev-dependencies]` table.
pub(super) fn is_dev_only(ws: &Workspace, krate: &str) -> bool {
    let mut found = false;
    for other in crate_dirs(ws) {
        let manifest = ws.manifest(&other);
        if manifest.dependencies().iter().any(|(key, _)| key == krate) {
            return false;
        }
        found |= manifest
            .dev_dependencies()
            .iter()
            .any(|(key, _)| key == krate);
    }
    found
}

/// Every crate directory the workspace holds.
fn crate_dirs(ws: &Workspace) -> Vec<String> {
    let mut dirs: BTreeSet<String> = BTreeSet::new();
    for (label, _) in ws.sources() {
        if let Some((krate, _)) = crate_of(label) {
            dirs.insert(krate.to_string());
        }
    }
    dirs.into_iter().collect()
}

/// The crate directory and crate-relative path of a source label.
pub(super) fn crate_of(label: &str) -> Option<(&str, &str)> {
    label.split_once('/')
}

// endregion: --- Functions
