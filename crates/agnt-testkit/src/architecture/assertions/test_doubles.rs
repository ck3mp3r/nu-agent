//! ARCH-5 (source half): no production source names a rig test double.
//!
//! ARCH-5 permits a mock at the LLM wire — that is how the E2E fabric runs the
//! same code path with a stub provider. It does not permit a test double to reach
//! production through a re-export or a type position, so the census pins every
//! `rig::test_utils` reference a production source carries.
//!
//! The parse decides the site. A local module that happens to be named
//! `test_utils` is not a rig reference, a path that begins elsewhere never fires,
//! and a mention in a doc comment or a string literal is not a path.

use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{ItemUse, Path, UseTree};

use crate::architecture::report::Violation;
use crate::architecture::sources::Workspace;

use super::support::labels;

// region:    --- Functions

/// WHEN a production source names `rig::test_utils`, THEN a test double has
/// reached production and the assertion SHALL report the reference's site.
///
/// Both shapes are read: a use tree (`pub use rig::test_utils::…`) and a path in
/// type position (a payload typed as a mock model). A reference is a reference
/// wherever it is written, so the census does not distinguish the re-export that
/// smuggles the double out from the variant that names it at home.
pub fn arch_5_rig_test_doubles(ws: &Workspace) -> Vec<Violation> {
    let mut violations = Vec::new();
    for label in labels(ws) {
        let Some(file) = ws.rust_file(label) else {
            continue;
        };
        let mut sites = Sites::default();
        sites.visit_file(&file);
        for (line, path) in sites.found {
            violations.push(Violation {
                assertion: ASSERTION,
                site: format!("{label}:{line}"),
                detail: format!("`{path}` is a rig test double"),
            });
        }
    }
    violations
}

// endregion: --- Functions

// region:    --- Support

/// The assertion this module reports.
const ASSERTION: &str = "ARCH-5";

/// The crate whose test utilities the census counts.
const RIG: &str = "rig";

/// The module inside `rig` that holds the test doubles.
const TEST_UTILS: &str = "test_utils";

/// The references one file carries, as (line, rendered path).
#[derive(Default)]
struct Sites {
    found: Vec<(usize, String)>,
}

impl Sites {
    /// Record `segments` at `line` when they open with `rig::test_utils`.
    fn record(&mut self, segments: &[String], line: usize) {
        if !is_rig_test_utils(segments) {
            return;
        }
        self.found.push((line, segments.join("::")));
    }
}

impl<'ast> Visit<'ast> for Sites {
    fn visit_item_use(&mut self, node: &'ast ItemUse) {
        let mut segments = Vec::new();
        flatten_use_tree(&node.tree, &mut segments);
        self.record(&segments, node.use_token.span.start().line);
        visit::visit_item_use(self, node);
    }

    fn visit_path(&mut self, node: &'ast Path) {
        let segments: Vec<String> = node
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect();
        self.record(&segments, node.span().start().line);
        visit::visit_path(self, node);
    }
}

/// Whether `segments` open with [`RIG`] and [`TEST_UTILS`].
fn is_rig_test_utils(segments: &[String]) -> bool {
    let mut segments = segments.iter();
    segments.next().is_some_and(|segment| segment == RIG)
        && segments.next().is_some_and(|segment| segment == TEST_UTILS)
}

/// Flatten a use tree into the segments it names.
///
/// A group is one segment in the rendering: `use rig::test_utils::{A, B}` names
/// the module once, and the members are its contents, not further path segments.
fn flatten_use_tree(tree: &UseTree, out: &mut Vec<String>) {
    match tree {
        UseTree::Path(use_path) => {
            out.push(use_path.ident.to_string());
            flatten_use_tree(&use_path.tree, out);
        }
        UseTree::Name(name) => out.push(name.ident.to_string()),
        UseTree::Rename(rename) => out.push(rename.ident.to_string()),
        UseTree::Glob(_) => out.push("*".to_string()),
        UseTree::Group(_) => out.push("{…}".to_string()),
    }
}

// endregion: --- Support
