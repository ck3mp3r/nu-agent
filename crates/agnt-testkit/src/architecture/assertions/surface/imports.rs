//! The `use`-tree resolution: which crate each name a file imports binds to.

// region:    --- Modules

use std::collections::{BTreeMap, BTreeSet};

use syn::visit::{self, Visit};
use syn::{ItemUse, UseTree};

// endregion: --- Modules

// region:    --- Functions

/// The crate each name one file imports binds to.
///
/// `use agnt_kernel::records::InferenceId;` binds `InferenceId` to
/// `agnt_kernel`, and a leaf rename (`use …::RequestId as Id;`) binds `Id`. Only
/// a leaf whose path starts with a workspace crate is recorded: `use crate::…`
/// and a standard-library path bind no cross-crate name.
pub(super) fn imported_crates(
    crates: &BTreeSet<String>,
    file: &syn::File,
) -> BTreeMap<String, String> {
    let mut imports = Imports {
        crates,
        found: BTreeMap::new(),
    };
    imports.visit_file(file);
    imports.found
}

/// Every name one file imports from a workspace crate, and the crate it binds.
struct Imports<'a> {
    crates: &'a BTreeSet<String>,
    found: BTreeMap<String, String>,
}

impl<'ast> Visit<'ast> for Imports<'_> {
    fn visit_item_use(&mut self, node: &'ast ItemUse) {
        for (segments, alias) in use_leaves(&node.tree) {
            let Some((krate, leaf)) = segments
                .split_first()
                .and_then(|(krate, rest)| rest.last().map(|leaf| (krate, leaf)))
            else {
                continue;
            };
            if !self.crates.contains(krate) {
                continue;
            }
            let bound = alias.unwrap_or_else(|| leaf.clone());
            self.found.entry(bound).or_insert_with(|| krate.clone());
        }
        visit::visit_item_use(self, node);
    }
}

/// Every leaf a use tree names, as the path it takes and the alias it binds.
fn use_leaves(tree: &UseTree) -> Vec<(Vec<String>, Option<String>)> {
    let mut leaves = Vec::new();
    collect_use_leaves(tree, &mut Vec::new(), &mut leaves);
    leaves
}

/// Collect the leaves of one use tree, carrying the path that leads to them.
fn collect_use_leaves(
    tree: &UseTree,
    prefix: &mut Vec<String>,
    out: &mut Vec<(Vec<String>, Option<String>)>,
) {
    match tree {
        UseTree::Path(use_path) => {
            prefix.push(use_path.ident.to_string());
            collect_use_leaves(&use_path.tree, prefix, out);
            prefix.pop();
        }
        UseTree::Name(name) => {
            let mut segments = prefix.clone();
            segments.push(name.ident.to_string());
            out.push((segments, None));
        }
        UseTree::Rename(rename) => {
            let mut segments = prefix.clone();
            segments.push(rename.ident.to_string());
            out.push((segments, Some(rename.rename.to_string())));
        }
        UseTree::Group(group) => {
            for item in &group.items {
                collect_use_leaves(item, prefix, out);
            }
        }
        UseTree::Glob(_) => {}
    }
}

/// Flatten a use tree into the segments it names, group members included.
pub(super) fn flatten_use_tree(tree: &UseTree, out: &mut Vec<String>) {
    match tree {
        UseTree::Path(use_path) => {
            out.push(use_path.ident.to_string());
            flatten_use_tree(&use_path.tree, out);
        }
        UseTree::Name(name) => out.push(name.ident.to_string()),
        UseTree::Rename(rename) => out.push(rename.ident.to_string()),
        UseTree::Glob(_) => out.push("*".to_string()),
        UseTree::Group(group) => {
            for item in &group.items {
                flatten_use_tree(item, out);
            }
        }
    }
}

// endregion: --- Functions
