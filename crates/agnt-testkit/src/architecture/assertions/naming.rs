//! ARCH-10: no consumer names the aggregate runtime type.
//!
//! The aggregate exists where it is assembled and in the production alias.
//! A consumer takes the narrow trait it uses, so the check reads parsed paths —
//! doc comments are attribute string literals, never paths, and a type whose
//! name merely starts with the aggregate's is a different type.

use syn::visit::{self, Visit};
use syn::{Path, UseTree};

use crate::architecture::report::Violation;
use crate::architecture::sources::Workspace;

use super::support::labels;

// region:    --- Functions

/// WHEN a file outside the composition root names the aggregate runtime type,
/// THEN the consumer SHALL name a trait instead and the assertion SHALL report
/// the reference.
///
/// The match is on the path's last segment ident exactly: `AgentRuntimeBuilder`
/// is a different type and never fires, and a doc comment naming the aggregate
/// is a string literal, so it never fires either.
pub fn arch_10_aggregate_runtime_references(ws: &Workspace) -> Vec<Violation> {
    let mut violations = Vec::new();
    for label in labels(ws) {
        if ROOT_FILES.contains(&label) {
            continue;
        }
        let Some(file) = ws.rust_file(label) else {
            continue;
        };
        let mut names = Names::default();
        names.visit_file(&file);
        for (line, name) in names.found {
            violations.push(Violation {
                assertion: ASSERTION_NAMING,
                site: format!("{label}:{line}"),
                detail: name,
            });
        }
    }
    violations
}

// endregion: --- Functions

// region:    --- Support

/// The assertion a consumer naming the aggregate breaks.
const ASSERTION_NAMING: &str = "ARCH-10";

/// The aggregate's names: the struct and the production alias.
const AGGREGATE_NAMES: [&str; 2] = ["AgentRuntime", "AgentConversationRuntime"];

/// The composition root's own files: the two that assemble the aggregate.
const ROOT_FILES: [&str; 2] = [
    "agnt-runtime/src/conversation/runtime.rs",
    "agnt-runtime/src/conversation/runtime_build.rs",
];

/// The aggregate names one file carries.
#[derive(Default)]
struct Names {
    found: Vec<(usize, String)>,
}

impl<'ast> Visit<'ast> for Names {
    fn visit_path(&mut self, node: &'ast Path) {
        if let Some(last) = node.segments.last() {
            let name = last.ident.to_string();
            if AGGREGATE_NAMES.contains(&name.as_str()) {
                self.found.push((last.ident.span().start().line, name));
            }
        }
        visit::visit_path(self, node);
    }

    /// The import that brings the aggregate into a file names it too, and its
    /// tree carries an `Ident`, not a `Path` — so the leaf is matched here.
    fn visit_use_tree(&mut self, node: &'ast UseTree) {
        let ident = match node {
            UseTree::Name(name) => Some(&name.ident),
            UseTree::Rename(rename) => Some(&rename.ident),
            _ => None,
        };
        if let Some(ident) = ident {
            let name = ident.to_string();
            if AGGREGATE_NAMES.contains(&name.as_str()) {
                self.found.push((ident.span().start().line, name));
            }
        }
        visit::visit_use_tree(self, node);
    }
}

// endregion: --- Support
