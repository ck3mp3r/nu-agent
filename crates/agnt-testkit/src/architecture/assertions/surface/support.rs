//! The partition and the reported row: what every walk in this module shares.

// region:    --- Modules

use std::collections::BTreeMap;

use proc_macro2::TokenTree;
use syn::{Attribute, Item, Meta, Visibility};

use super::evidence::{
    called_names, consumed_names, cross_crate_method_names, cross_crate_references, is_dev_only,
};

use crate::architecture::report::Violation;
use crate::architecture::sources::Workspace;

// endregion: --- Modules

// region:    --- Support

/// The assertion this module reports.
const ASSERTION: &str = "ARCH-6";

/// One public item: its source label, its declaration line and its name.
pub(super) type SurfaceItem = (String, usize, String);

/// The violations `surface` yields: every recorded declaration no other crate
/// reaches, keyed per crate as the baseline files are.
pub(super) fn report(
    ws: &Workspace,
    surface: BTreeMap<String, Vec<SurfaceItem>>,
) -> Vec<Violation> {
    let references = cross_crate_references(ws);
    let methods = cross_crate_method_names(ws);
    let mut violations = Vec::new();
    for (krate, items) in &surface {
        if is_dev_only(ws, krate) {
            continue;
        }
        let mut consumed = consumed_names(&references, &krate.replace('-', "_"));
        consumed.extend(called_names(&methods, &krate.replace('-', "_")));
        for (label, line, name) in items {
            if consumed.contains(name) {
                continue;
            }
            violations.push(Violation {
                assertion: ASSERTION,
                site: format!("{label}:{line}"),
                detail: format!("`{name}` has no consumer outside `{krate}`"),
            });
        }
    }
    violations
}

/// Whether the visibility is plain `pub`.
pub(super) fn is_public(vis: &Visibility) -> bool {
    matches!(vis, Visibility::Public(_))
}

/// The attributes an item carries.
pub(super) fn attrs_of(item: &Item) -> &[Attribute] {
    match item {
        Item::Fn(item) => &item.attrs,
        Item::Struct(item) => &item.attrs,
        Item::Enum(item) => &item.attrs,
        Item::Trait(item) => &item.attrs,
        Item::Type(item) => &item.attrs,
        Item::Const(item) => &item.attrs,
        Item::Static(item) => &item.attrs,
        Item::Mod(item) => &item.attrs,
        Item::Use(item) => &item.attrs,
        _ => &[],
    }
}

/// Whether the item sits behind `#[cfg(test)]` or another `cfg` naming `test`.
pub(super) fn is_test_gated(attrs: &[Attribute]) -> bool {
    attrs
        .iter()
        .filter(|attr| attr.path().is_ident("cfg"))
        .any(|attr| {
            let Meta::List(list) = &attr.meta else {
                return false;
            };
            list.tokens
                .clone()
                .into_iter()
                .any(|token| matches!(token, TokenTree::Ident(ident) if ident == "test"))
        })
}

// endregion: --- Support
