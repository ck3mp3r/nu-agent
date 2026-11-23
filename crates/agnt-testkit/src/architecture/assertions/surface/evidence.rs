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
///
/// Both keys are underscored: the consumed crate is recorded from the path's
/// first segment, which is the Rust crate name, and the consumer is normalized
/// here so [`consumed_names`]'s self-exclusion and [`called_names`]'s gate read
/// the same key. Keyed by the crate directory's dashed name, a consumer's own
/// paths would look like an outside consumer's.
///
/// The consumer key names a target, not a directory: see [`consumer_key`].
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
        let Some(consumer) = consumer_key(label) else {
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
                .entry((consumer.clone(), target))
                .or_default()
                .extend(names);
        }
    }
    references
}

/// The names `krate` publishes that some other crate reaches.
///
/// Both sides of the key are underscored, so `consumer != krate` is the
/// self-exclusion it reads as: a crate's own paths into itself are not evidence
/// of an outside consumer.
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
/// package's underscored name.
///
/// The key is normalized the same way [`called_names`] excludes the declaring
/// crate: a crate's own receiver calls (`self.take_pending_events()`) are not
/// cross-crate evidence, and keying by the crate directory's dashed name would
/// make every one of them look like an outside consumer.
///
/// The key is the package's, never a target's: a receiver call names no crate
/// segment, so the gate admits a caller's names by the references the package
/// writes, and the package is the identity that gate reads. A separate bin key
/// here would admit nothing new — the bin could only lose its names.
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

/// The names `krate`'s methods are called by, from a crate that also references
/// `krate`.
///
/// A receiver call carries no crate segment: `<expr>.<name>(...)` records only
/// `name`, so the call alone cannot tell which crate's type the receiver is. The
/// gate is a cross-crate path — when `caller` references `krate`, the gate
/// admits its method names; when it names no path into `krate`, it admits
/// nothing. A same-named method call in a crate that references `krate` still
/// credits `krate`'s row: the residual over-count the review entry records.
pub(super) fn called_names(
    names: &BTreeMap<String, BTreeSet<String>>,
    krate: &str,
    references: &BTreeMap<(String, String), BTreeSet<String>>,
) -> BTreeSet<String> {
    let mut called = BTreeSet::new();
    for (caller, methods) in names {
        if caller == krate {
            continue;
        }
        if references.contains_key(&(caller.to_string(), krate.to_string())) {
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

/// The consumer key the references of the source at `label` are attributed to.
///
/// A package's `src/main.rs` and `src/bin/` files compile into a binary target
/// that names the package's library as a dependency. The bin is therefore not the
/// library, and the two take different keys: the bin `<crate>#bin`, the library
/// `<crate>`. Keyed by the directory alone, the bin's paths into its own library
/// read as the library's own paths, and [`consumed_names`]'s self-exclusion
/// discards them — real consumption attributed to no consumer.
///
/// The two keys cannot collide: a crate name carries no `#`, so `<crate>#bin`
/// names no crate.
fn consumer_key(label: &str) -> Option<String> {
    let (krate, relative) = crate_of(label)?;
    let krate = krate.replace('-', "_");
    if is_bin_target(relative) {
        return Some(format!("{krate}#bin"));
    }
    Some(krate)
}

/// Whether the crate-relative source path belongs to the package's binary
/// target: its `src/main.rs`, or a file under its `src/bin/` directory.
fn is_bin_target(relative: &str) -> bool {
    relative == "src/main.rs" || relative.starts_with("src/bin/")
}

// endregion: --- Functions
