//! The declaration walks: every `pub` item a crate declares, and every `pub`
//! member of an inherent impl whose self type the crate surfaces.

// region:    --- Modules

use std::collections::{BTreeMap, BTreeSet};

use syn::{ImplItem, Item, Type, UseTree, Visibility};

use super::evidence::crate_of;
use super::support::{SurfaceItem, attrs_of, is_public, is_test_gated};

use crate::architecture::sources::Workspace;

// endregion: --- Modules

// region:    --- Functions

/// Every public item a crate declares in its `src`, keyed by crate directory.
pub(super) fn surface(ws: &Workspace) -> BTreeMap<String, Vec<SurfaceItem>> {
    let mut surface: BTreeMap<String, Vec<SurfaceItem>> = BTreeMap::new();
    for (label, _) in ws.sources() {
        let Some(file) = ws.rust_file(label) else {
            continue;
        };
        let Some((krate, _)) = crate_of(label) else {
            continue;
        };
        let mut items = Vec::new();
        collect_surface(&file.items, label, true, &mut items);
        surface.entry(krate.to_string()).or_default().extend(items);
    }
    surface
}

/// Every public item in `items`, walking inline modules and carrying whether the
/// module chain is public.
fn collect_surface(items: &[Item], label: &str, public: bool, out: &mut Vec<SurfaceItem>) {
    for item in items {
        if is_test_gated(attrs_of(item)) {
            continue;
        }
        match item {
            Item::Mod(item_mod) => {
                if let Some((_, nested)) = &item_mod.content {
                    let nested_public = public && is_public(&item_mod.vis);
                    collect_surface(nested, label, nested_public, out);
                }
            }
            Item::Use(_) => {}
            _ => {
                if !public {
                    continue;
                }
                let Some((vis, name, line)) = item_declaration(item) else {
                    continue;
                };
                if is_public(vis) {
                    out.push((label.to_string(), line, name));
                }
            }
        }
    }
}

/// The visibility, name and declaration line of a named item.
fn item_declaration(item: &Item) -> Option<(&Visibility, String, usize)> {
    match item {
        Item::Fn(item_fn) => Some((
            &item_fn.vis,
            item_fn.sig.ident.to_string(),
            item_fn.sig.ident.span().start().line,
        )),
        Item::Struct(item_struct) => Some((
            &item_struct.vis,
            item_struct.ident.to_string(),
            item_struct.ident.span().start().line,
        )),
        Item::Enum(item_enum) => Some((
            &item_enum.vis,
            item_enum.ident.to_string(),
            item_enum.ident.span().start().line,
        )),
        Item::Trait(item_trait) => Some((
            &item_trait.vis,
            item_trait.ident.to_string(),
            item_trait.ident.span().start().line,
        )),
        Item::Type(item_type) => Some((
            &item_type.vis,
            item_type.ident.to_string(),
            item_type.ident.span().start().line,
        )),
        Item::Const(item_const) => Some((
            &item_const.vis,
            item_const.ident.to_string(),
            item_const.ident.span().start().line,
        )),
        Item::Static(item_static) => Some((
            &item_static.vis,
            item_static.ident.to_string(),
            item_static.ident.span().start().line,
        )),
        _ => None,
    }
}

/// Every public impl member a crate declares on a census-visible type in its
/// `src`, keyed by crate directory.
pub(super) fn member_surface(ws: &Workspace) -> BTreeMap<String, Vec<SurfaceItem>> {
    let types = public_types(ws);
    let mut surface: BTreeMap<String, Vec<SurfaceItem>> = BTreeMap::new();
    for (label, _) in ws.sources() {
        let Some(file) = ws.rust_file(label) else {
            continue;
        };
        let Some((krate, _)) = crate_of(label) else {
            continue;
        };
        let Some(visible) = types.get(krate) else {
            continue;
        };
        let mut members = Vec::new();
        collect_members(&file.items, label, true, visible, &mut members);
        surface
            .entry(krate.to_string())
            .or_default()
            .extend(members);
    }
    surface
}

/// Every type name a crate surfaces publicly, keyed by crate directory: the
/// types it declares `pub` and the leaves its `pub use` trees name.
fn public_types(ws: &Workspace) -> BTreeMap<String, BTreeSet<String>> {
    let mut types: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (label, _) in ws.sources() {
        let Some(file) = ws.rust_file(label) else {
            continue;
        };
        let Some((krate, _)) = crate_of(label) else {
            continue;
        };
        let mut names = BTreeSet::new();
        collect_public_types(&file.items, true, &mut names);
        types.entry(krate.to_string()).or_default().extend(names);
    }
    types
}

/// Every type name in `items` that is publicly surfaced, walking inline modules
/// and carrying whether the module chain is public.
fn collect_public_types(items: &[Item], public: bool, out: &mut BTreeSet<String>) {
    for item in items {
        if is_test_gated(attrs_of(item)) {
            continue;
        }
        match item {
            Item::Mod(item_mod) => {
                if let Some((_, nested)) = &item_mod.content {
                    let nested_public = public && is_public(&item_mod.vis);
                    collect_public_types(nested, nested_public, out);
                }
            }
            Item::Use(item_use) => {
                if public && is_public(&item_use.vis) {
                    collect_use_names(&item_use.tree, out);
                }
            }
            _ => {
                if !public || !is_type_item(item) {
                    continue;
                }
                let Some((vis, name, _)) = item_declaration(item) else {
                    continue;
                };
                if is_public(vis) {
                    out.insert(name);
                }
            }
        }
    }
}

/// The names a `pub use` tree surfaces: the leaf of a path, the alias of a
/// rename, and the leaves of a group. A glob names no leaf, so it adds nothing.
fn collect_use_names(tree: &UseTree, out: &mut BTreeSet<String>) {
    match tree {
        UseTree::Path(use_path) => collect_use_names(&use_path.tree, out),
        UseTree::Name(name) => {
            out.insert(name.ident.to_string());
        }
        UseTree::Rename(rename) => {
            out.insert(rename.rename.to_string());
        }
        UseTree::Group(group) => {
            for item in &group.items {
                collect_use_names(item, out);
            }
        }
        UseTree::Glob(_) => {}
    }
}

/// Whether the item declares a type — what an `impl` block can attach members
/// to.
fn is_type_item(item: &Item) -> bool {
    matches!(
        item,
        Item::Struct(_) | Item::Enum(_) | Item::Trait(_) | Item::Type(_)
    )
}

/// Every `pub` impl member in `items` whose self type `visible` names, walking
/// inline modules and carrying whether the module chain is public.
fn collect_members(
    items: &[Item],
    label: &str,
    public: bool,
    visible: &BTreeSet<String>,
    out: &mut Vec<SurfaceItem>,
) {
    for item in items {
        if is_test_gated(attrs_of(item)) {
            continue;
        }
        match item {
            Item::Mod(item_mod) => {
                if let Some((_, nested)) = &item_mod.content {
                    let nested_public = public && is_public(&item_mod.vis);
                    collect_members(nested, label, nested_public, visible, out);
                }
            }
            Item::Impl(item_impl) => {
                if !public {
                    continue;
                }
                let Some(self_type) = self_type_name(&item_impl.self_ty) else {
                    continue;
                };
                if !visible.contains(&self_type) {
                    continue;
                }
                for member in &item_impl.items {
                    let Some((vis, name, line)) = member_declaration(member) else {
                        continue;
                    };
                    if is_public(vis) {
                        out.push((label.to_string(), line, name));
                    }
                }
            }
            _ => {}
        }
    }
}

/// The visibility, name and declaration line of a named impl member.
fn member_declaration(member: &ImplItem) -> Option<(&Visibility, String, usize)> {
    match member {
        ImplItem::Fn(member_fn) => Some((
            &member_fn.vis,
            member_fn.sig.ident.to_string(),
            member_fn.sig.ident.span().start().line,
        )),
        ImplItem::Const(member_const) => Some((
            &member_const.vis,
            member_const.ident.to_string(),
            member_const.ident.span().start().line,
        )),
        ImplItem::Type(member_type) => Some((
            &member_type.vis,
            member_type.ident.to_string(),
            member_type.ident.span().start().line,
        )),
        _ => None,
    }
}

/// The type name an `impl` block attaches to: the last segment of a path type.
fn self_type_name(self_ty: &Type) -> Option<String> {
    let Type::Path(type_path) = self_ty else {
        return None;
    };
    type_path
        .path
        .segments
        .last()
        .map(|segment| segment.ident.to_string())
}

// endregion: --- Functions
