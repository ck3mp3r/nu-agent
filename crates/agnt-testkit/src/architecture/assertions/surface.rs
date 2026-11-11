//! ARCH-6 (census half): every declared public item has a consumer outside its
//! crate.
//!
//! The judgement half — "or is a documented entry point with a named caller" —
//! stays a review row. This half is the part a parser can decide: an item a
//! crate declares `pub` and no other crate's `src` reaches is surface nothing
//! consumes.
//!
//! An item is one declaration site. A `pub use` leaf is not a site: it aliases a
//! name the crate does not own, so the census reads the items the crate defines.
//!
//! Evidence is a cross-crate path. A consumer names the crate it reaches
//! (`agnt_storage::…`) and then the item, in a type position, an expression or a
//! use tree, so the census reads paths and never identifiers on their own: a
//! field named `transcript` in another crate is not a consumer of
//! `agnt_storage`'s `transcript()`. Same-crate use is not evidence either: the
//! property is about surface across a crate boundary.
//!
//! Blind spots, recorded rather than closed: a glob import
//! (`use agnt_runtime::types::*`) names no item, a macro-generated reference is
//! invisible to `syn`'s parsed tokens, and a crate reached through a `use … as …`
//! rename hides its paths behind the alias.
//!
//! One limit the record inherits, stated rather than hidden:
//!
//! - Reachability is not computed across files. An item is counted when its own
//!   visibility is `pub` and the inline module chain around it is public, so a
//!   `pub` item in a private submodule file is counted too — the census records
//!   declarations, which is what the assertion names. The member walk resolves an
//!   `impl` crate-wide, by self-type name against the crate's surfaced types, so
//!   the type's declaration file is not consulted.
//!
//! Two rules the walk applies, stated so a reader can check them against a row:
//!
//! - Associated functions are surface too. Two walks make one census: the item
//!   walk reads declarations, and the member walk
//!   ([`arch_6_impl_member_census`]) reads the `pub` members of inherent impls
//!   whose self type the crate surfaces — declares `pub` in a public module
//!   chain, or re-exports through a `pub use` leaf. A trait implementation's
//!   members carry no visibility of their own and are therefore outside both.
//! - Consumer evidence is a path segment. A consumer's module segment that
//!   happens to equal an item's name suppresses that item's row: a path into
//!   `agnt_storage::transcript` consumes a row named `transcript` whether or not
//!   the item is what the path reaches. Measured for the member half: 65 of 934
//!   members (7.0%) are suppressed this way.
//!
//! A crate that appears in no crate's `[dependencies]` table and in some crate's
//! `[dev-dependencies]` table is dev-only: its surface exists for test trees, so
//! the production-consumer rule does not apply to it.
//!
//! Record-then-drain: the recorded rows are a drain backlog, not compliance. The
//! ratchet bites on new surface only, and it only ever shrinks — a removed item's
//! row leaves the baseline in the same commit.

use std::collections::{BTreeMap, BTreeSet};

use proc_macro2::TokenTree;
use syn::visit::{self, Visit};
use syn::{Attribute, ImplItem, Item, ItemUse, Meta, Path, Type, UseTree, Visibility};

use crate::architecture::report::Violation;
use crate::architecture::sources::Workspace;

// region:    --- Functions

/// WHEN a crate declares a `pub` item and no other crate's `src` reaches it,
/// THEN the item SHALL have no cross-crate consumer and the assertion SHALL
/// report its declaration site, unless a baseline row records it.
pub fn arch_6_public_surface_consumer_census(ws: &Workspace) -> Vec<Violation> {
    report(ws, surface(ws))
}

/// WHEN a `pub` impl member on a census-visible type has no cross-crate
/// consumer, THEN the member SHALL be reported at its declaration site.
///
/// The member is the surface here, not the type: a type another crate reaches
/// does not consume its methods, so the member name is what must appear in a
/// consumer's path. A trait implementation's members carry no visibility of
/// their own and are therefore outside the walk; an inherent `impl`'s `pub`
/// members are the record's subject.
pub fn arch_6_impl_member_census(ws: &Workspace) -> Vec<Violation> {
    report(ws, member_surface(ws))
}

// endregion: --- Functions

// region:    --- Support

/// The assertion this module reports.
const ASSERTION: &str = "ARCH-6";

/// One public item: its source label, its declaration line and its name.
type SurfaceItem = (String, usize, String);

/// The violations `surface` yields: every recorded declaration no other crate
/// reaches, keyed per crate as the baseline files are.
fn report(ws: &Workspace, surface: BTreeMap<String, Vec<SurfaceItem>>) -> Vec<Violation> {
    let references = cross_crate_references(ws);
    let mut violations = Vec::new();
    for (krate, items) in &surface {
        if is_dev_only(ws, krate) {
            continue;
        }
        let consumed = consumed_names(&references, &krate.replace('-', "_"));
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

/// Every public item a crate declares in its `src`, keyed by crate directory.
fn surface(ws: &Workspace) -> BTreeMap<String, Vec<SurfaceItem>> {
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
fn member_surface(ws: &Workspace) -> BTreeMap<String, Vec<SurfaceItem>> {
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

/// Every cross-crate path the workspace's sources write, keyed by (consumer
/// crate, consumed crate).
fn cross_crate_references(ws: &Workspace) -> BTreeMap<(String, String), BTreeSet<String>> {
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
        let mut paths = Paths {
            crates: &crates,
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
fn consumed_names(
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

/// Whether `krate` is dev-only: named in no `[dependencies]` table and in at
/// least one `[dev-dependencies]` table.
fn is_dev_only(ws: &Workspace, krate: &str) -> bool {
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
fn crate_of(label: &str) -> Option<(&str, &str)> {
    label.split_once('/')
}

/// Whether the visibility is plain `pub`.
fn is_public(vis: &Visibility) -> bool {
    matches!(vis, Visibility::Public(_))
}

/// The attributes an item carries.
fn attrs_of(item: &Item) -> &[Attribute] {
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
fn is_test_gated(attrs: &[Attribute]) -> bool {
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

/// Every crate path one file writes, grouped by the crate it names.
struct Paths<'a> {
    crates: &'a BTreeSet<String>,
    found: BTreeMap<String, BTreeSet<String>>,
}

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
}

impl<'ast> Visit<'ast> for Paths<'_> {
    fn visit_path(&mut self, node: &'ast Path) {
        let segments: Vec<String> = node
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect();
        self.record(&segments);
        visit::visit_path(self, node);
    }

    fn visit_item_use(&mut self, node: &'ast ItemUse) {
        let mut segments = Vec::new();
        flatten_use_tree(&node.tree, &mut segments);
        self.record(&segments);
        visit::visit_item_use(self, node);
    }
}

/// Flatten a use tree into the segments it names, group members included.
fn flatten_use_tree(tree: &UseTree, out: &mut Vec<String>) {
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

// endregion: --- Support
