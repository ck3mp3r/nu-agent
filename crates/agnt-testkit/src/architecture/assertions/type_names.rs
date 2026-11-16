//! The type-name marker check: a type name says what the type is, never its
//! implementation status.
//!
//! The naming law bans a name that encodes shape — `docs/design-choices.md` §6
//! records the `*_impl` form. The type-name face of it is a CamelCase segment:
//! `PeerDiscoveryImpl` carries `Impl`, and `Simple` does not, though it holds the
//! same letters.
//!
//! The subject is a parsed declaration, so the suite's syn rule applies and is
//! satisfied: the name and its line come from the declaration's span.
//!
//! Zero tolerance, deliberately: a type name is cheap to fix, and a type-name
//! ledger would reintroduce the self-service growth hole the baseline direction
//! exists to close. The segment list is maintained by ruling against a clean tree,
//! and a violation has no recording path.

use syn::visit::{self, Visit};
use syn::{Ident, ItemEnum, ItemStruct, ItemTrait, ItemType};

use crate::architecture::report::Violation;
use crate::architecture::sources::Workspace;

// region:    --- Functions

/// WHEN a parsed type declaration's name carries one of the banned name
/// segments, THEN the check SHALL report the declaration site.
pub fn marked_type_names(ws: &Workspace) -> Vec<Violation> {
    let mut violations = Vec::new();
    for (label, _) in ws.sources() {
        let Some(file) = ws.rust_file(label) else {
            continue;
        };
        let mut names = TypeNames::default();
        names.visit_file(&file);
        for (line, name, segment) in names.found {
            violations.push(Violation {
                assertion: ASSERTION_TYPES,
                site: format!("{label}:{line}"),
                detail: format!("`{name}` carries the `{segment}` name segment"),
            });
        }
    }
    violations
}

// endregion: --- Functions

// region:    --- Support

/// The assertion this module reports.
const ASSERTION_TYPES: &str = "TYPES";

/// The banned type-name segments, maintained like the vocabulary term lists: a
/// segment joins by ruling against a clean tree.
const MARKERS: [&str; 1] = ["Impl"];

/// The type names one file declares that carry a banned segment: the line, the
/// declared name, and the segment it carries.
type MarkedName = (usize, String, &'static str);

/// The type names one file declares that carry a banned segment.
#[derive(Default)]
struct TypeNames {
    found: Vec<MarkedName>,
}

impl TypeNames {
    /// Record `ident` when it carries a banned segment.
    fn record(&mut self, ident: &Ident) {
        let name = ident.to_string();
        if let Some(segment) = MARKERS
            .into_iter()
            .find(|segment| carries_segment(&name, segment))
        {
            self.found.push((ident.span().start().line, name, segment));
        }
    }
}

impl<'ast> Visit<'ast> for TypeNames {
    fn visit_item_struct(&mut self, node: &'ast ItemStruct) {
        self.record(&node.ident);
        visit::visit_item_struct(self, node);
    }

    fn visit_item_enum(&mut self, node: &'ast ItemEnum) {
        self.record(&node.ident);
        visit::visit_item_enum(self, node);
    }

    fn visit_item_trait(&mut self, node: &'ast ItemTrait) {
        self.record(&node.ident);
        visit::visit_item_trait(self, node);
    }

    fn visit_item_type(&mut self, node: &'ast ItemType) {
        self.record(&node.ident);
        visit::visit_item_type(self, node);
    }
}

/// Whether `name` carries `word` as a CamelCase segment.
///
/// A segment begins at an uppercase character and ends at the next one, so
/// `PeerDiscoveryImpl` is `Peer`, `Discovery` and `Impl`, while `Simple` is one
/// segment and never matches. The rule is the word, never the letters.
fn carries_segment(name: &str, word: &str) -> bool {
    name.char_indices()
        .filter(|(_, character)| character.is_uppercase())
        .any(|(start, _)| {
            let rest = &name[start..];
            let end = rest
                .char_indices()
                .skip(1)
                .find(|(_, character)| character.is_uppercase())
                .map_or(rest.len(), |(index, _)| index);
            &rest[..end] == word
        })
}

// endregion: --- Support
