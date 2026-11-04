//! ARCH-4: one type per distinction, measured as duplicate `pub enum` names.

use std::collections::BTreeMap;

use syn::{Item, Visibility};

use crate::architecture::report::Violation;
use crate::architecture::sources::Workspace;

// region:    --- Functions

/// WHEN two `pub enum` declarations in the workspace share a name, THEN the
/// distinction SHALL have more than one type and the assertion SHALL report
/// every declaration of that name.
///
/// The assertion counts, it does not judge: which duplicate is legitimate is a
/// later slice's claim. A name declared once, a `pub(crate)` declaration and a
/// declaration nested inside a function body are all outside the census.
pub fn arch_4_duplicate_enum_names(ws: &Workspace) -> Vec<Violation> {
    let mut groups: BTreeMap<String, Vec<(String, usize)>> = BTreeMap::new();
    for (label, _) in ws.sources() {
        let Some(file) = ws.rust_file(label) else {
            continue;
        };
        for item in walk(&file.items) {
            let Item::Enum(item_enum) = item else {
                continue;
            };
            if !matches!(item_enum.vis, Visibility::Public(_)) {
                continue;
            }
            let line = item_enum.ident.span().start().line;
            groups
                .entry(item_enum.ident.to_string())
                .or_default()
                .push((label.to_string(), line));
        }
    }

    let mut violations = Vec::new();
    for (name, sites) in groups {
        let declared = sites.len();
        if declared < 2 {
            continue;
        }
        violations.extend(sites.into_iter().map(|(label, line)| Violation {
            assertion: ASSERTION,
            site: format!("{label}:{line}"),
            detail: format!("`{name}` is declared {declared} times in the workspace"),
        }));
    }
    violations
}

// endregion: --- Functions

// region:    --- Support

/// The assertion this module reports.
const ASSERTION: &str = "ARCH-4";

/// Every item in `items`, with the items of inline modules included.
fn walk(items: &[Item]) -> Vec<&Item> {
    let mut out = Vec::new();
    for item in items {
        out.push(item);
        if let Item::Mod(item_mod) = item
            && let Some((_, nested)) = &item_mod.content
        {
            out.extend(walk(nested));
        }
    }
    out
}

// endregion: --- Support
