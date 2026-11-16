//! The per-file walk that records every crate path it meets.

// region:    --- Modules

use std::collections::{BTreeMap, BTreeSet};

use syn::visit::{self, Visit};
use syn::{Expr, ExprCall, ItemUse, Path};

use super::imports::flatten_use_tree;

// endregion: --- Modules

// region:    --- Types

/// Every crate path one file writes, grouped by the crate it names.
///
/// `imports` is what lets a call reach a crate without a crate segment:
/// `InferenceId::mint()` names the crate only through the file's `use` tree.
pub(super) struct Paths<'a> {
    pub(super) crates: &'a BTreeSet<String>,
    pub(super) imports: &'a BTreeMap<String, String>,
    pub(super) found: BTreeMap<String, BTreeSet<String>>,
}

/// Every method name one file calls through a receiver.
///
/// The call is `receiver.method(args)`: `syn` parses it as a method call whose
/// method is the tail of the field expression the receiver carries. Only the
/// method name is kept — the receiver's type is not resolved, because the census
/// asks whether any other crate names the surface, not which value it reaches.
#[derive(Default)]
pub(super) struct MethodCalls {
    pub(super) found: BTreeSet<String>,
}

// endregion: --- Types

// region:    --- Impls

impl Paths<'_> {
    /// Record `segments` when they name a crate: the first segment is the crate
    /// and the rest are the names reached through it.
    fn record(&mut self, segments: &[String]) {
        let Some((first, rest)) = segments.split_first() else {
            return;
        };
        if !self.crates.contains(first) {
            return;
        }
        self.found
            .entry(first.clone())
            .or_default()
            .extend(rest.iter().cloned());
    }

    /// Record `segments` when its first segment is a name the file imported: the
    /// rest of the path is what the import carries, attributed to the crate it
    /// was imported from. A one-segment path names the import alone and reaches
    /// no member.
    fn record_imported(&mut self, segments: &[String]) {
        let Some((first, rest)) = segments.split_first() else {
            return;
        };
        if rest.is_empty() {
            return;
        }
        let Some(krate) = self.imports.get(first) else {
            return;
        };
        self.found
            .entry(krate.clone())
            .or_default()
            .extend(rest.iter().cloned());
    }
}

impl<'ast> Visit<'ast> for Paths<'_> {
    fn visit_path(&mut self, node: &'ast Path) {
        self.record(&path_segments(node));
        visit::visit_path(self, node);
    }

    fn visit_item_use(&mut self, node: &'ast ItemUse) {
        let mut segments = Vec::new();
        flatten_use_tree(&node.tree, &mut segments);
        self.record(&segments);
        visit::visit_item_use(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if let Expr::Path(expr_path) = &*node.func {
            self.record_imported(&path_segments(&expr_path.path));
        }
        visit::visit_expr_call(self, node);
    }
}

impl<'ast> Visit<'ast> for MethodCalls {
    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        self.found.insert(node.method.to_string());
        visit::visit_expr_method_call(self, node);
    }
}

// endregion: --- Impls

// region:    --- Support

/// The segments a path names, in order.
fn path_segments(path: &Path) -> Vec<String> {
    path.segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect()
}

// endregion: --- Support
