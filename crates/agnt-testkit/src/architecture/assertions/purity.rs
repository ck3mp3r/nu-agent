//! ARCH-2: the rendering and orchestration vocabularies stay in their own crates.
//!
//! The property is one crate edge wide in each direction: `agnt-runtime` carries
//! no rendering vocabulary, and `agnt-render` carries no orchestration
//! vocabulary. The check reads each crate's **public API** — `pub` item names,
//! `pub use` leaves, `pub` free-function signatures, the field types of a `pub`
//! struct or enum, and the signatures of a `pub` trait's methods and of a `pub`
//! type's inherent methods — because that is the surface a crate hands across its
//! edge. A record folded inside a body is not the edge.
//!
//! Two limits are stated rather than covered. A name a macro expands to is not
//! in the parsed tree, so it is unmeasured. An inherent impl resolves its self
//! type file-locally: an `impl` in a different file from its `pub` type is not
//! recognised as the edge. Matching is exact, so a compound name built from an
//! excluded word (`MarkdownSection`) is not a term.
//!
//! The orchestration terms are the settled bus vocabulary of
//! `docs/bus-channels.md`: the six concern names, the six payload families, and
//! the `Bus` type they are reached through. The rendering terms are the drawer's
//! published vocabulary, measured at landing; three shared English words
//! (`Theme`, `Markdown`, `Display`) are left out on purpose, because they also
//! name a config concept and a `std` trait and would fire on a crate's own
//! vocabulary rather than on a leak.

use syn::visit::{self, Visit};
use syn::{FnArg, Item, ReturnType, Type, UseTree, Visibility};

use crate::architecture::report::Violation;
use crate::architecture::sources::Workspace;

// region:    --- Functions

/// WHEN `agnt-runtime` names rendering vocabulary, OR `agnt-render` names
/// orchestration vocabulary, THEN the crate SHALL keep to its own vocabulary and
/// the assertion SHALL report the site.
///
/// The scan is by symbol, never by text: a word in a doc comment or a string
/// literal is not a name, so a document that mentions a channel is not a
/// violation and a syntax-highlighting scope is not one either.
pub fn arch_2_vocabulary_purity(ws: &Workspace) -> Vec<Violation> {
    let mut violations = Vec::new();
    for (label, _) in ws.sources() {
        let terms: &[&str] = if label.starts_with(RUNTIME_PREFIX) {
            &RENDERING_TERMS
        } else if label.starts_with(RENDER_PREFIX) {
            &ORCHESTRATION_TERMS
        } else {
            continue;
        };
        let Some(file) = ws.rust_file(label) else {
            continue;
        };
        let pub_types = public_type_names(&file);
        let mut api = PublicApi {
            label,
            terms,
            pub_types: &pub_types,
            found: Vec::new(),
        };
        api.visit_file(&file);
        violations.extend(api.found);
    }
    violations
}

// endregion: --- Functions

// region:    --- Support

/// The assertion this module reports.
const ASSERTION: &str = "ARCH-2";

/// The crates, by their source labels.
const RUNTIME_PREFIX: &str = "agnt-runtime/src/";
const RENDER_PREFIX: &str = "agnt-render/src/";

/// The bus vocabulary, read off `docs/bus-channels.md`.
///
/// The six concern names, the six payload families that ride them, and the type
/// that carries the concerns. A drawer naming any of these names the
/// orchestration side of the workspace.
const ORCHESTRATION_TERMS: [&str; 13] = [
    // the six concerns
    "stream",
    "cancel",
    "control",
    "lifecycle",
    "narration",
    "facts",
    // the six payload families
    "SessionRecord",
    "NarrationEvent",
    "FactRecord",
    "LifecycleEvent",
    "ControlCommand",
    "CancelEvent",
    // the type the concerns are reached through
    "Bus",
];

/// The drawer's published vocabulary.
///
/// Distinctive names only: each is a term no runtime file carries today, so a
/// hit is a leak rather than a shared word.
const RENDERING_TERMS: [&str; 13] = [
    "RenderBackend",
    "RowRect",
    "LayoutPlanner",
    "EntryFamily",
    "FramePlan",
    "PlannedEntry",
    "PlainBackend",
    "ToolView",
    "ContentKind",
    "DisplaySection",
    "DisplayStats",
    "TranscriptEntry",
    "ItemStatus",
];

/// Collects the names one crate's public API carries that the term list bans.
struct PublicApi<'a> {
    label: &'a str,
    terms: &'a [&'a str],
    /// The `pub` type names declared in this file: an inherent impl's self type is
    /// resolved against them, file-locally.
    pub_types: &'a [String],
    found: Vec<Violation>,
}

impl PublicApi<'_> {
    /// Record `ident` when the term list bans it.
    fn check(&mut self, ident: &syn::Ident) {
        let name = ident.to_string();
        if !self.terms.contains(&name.as_str()) {
            return;
        }
        self.found.push(Violation {
            assertion: ASSERTION,
            site: format!("{label}:{}", ident.span().start().line, label = self.label),
            detail: format!("public API names `{name}`"),
        });
    }

    /// Record the last path segment of `ty`, the name an edge reader sees.
    fn check_type(&mut self, ty: &Type) {
        for ident in type_idents(ty) {
            self.check(ident);
        }
    }

    /// Record every banned name in `sig`: the parameter types and the return type.
    fn check_signature(&mut self, sig: &syn::Signature) {
        for input in &sig.inputs {
            let FnArg::Typed(pat_type) = input else {
                continue;
            };
            self.check_type(&pat_type.ty);
        }
        if let ReturnType::Type(_, ty) = &sig.output {
            self.check_type(ty);
        }
    }

    /// Whether `ty` names a `pub` type declared in this file.
    fn is_local_public_type(&self, ty: &Type) -> bool {
        let Type::Path(path) = ty else {
            return false;
        };
        path.path.segments.last().is_some_and(|segment| {
            self.pub_types
                .iter()
                .any(|name| name == &segment.ident.to_string())
        })
    }
}

impl<'ast> Visit<'ast> for PublicApi<'_> {
    fn visit_item(&mut self, node: &'ast Item) {
        if is_public(node) {
            if let Some(ident) = item_ident(node) {
                self.check(ident);
            }
            if let Item::Use(item_use) = node {
                collect_use_leaves(&item_use.tree, &mut |ident| self.check(ident));
            }
        }
        visit::visit_item(self, node);
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        if matches!(node.vis, Visibility::Public(_)) {
            for input in &node.sig.inputs {
                let FnArg::Typed(pat_type) = input else {
                    continue;
                };
                self.check_type(&pat_type.ty);
            }
            if let ReturnType::Type(_, ty) = &node.sig.output {
                self.check_type(ty);
            }
        }
        visit::visit_item_fn(self, node);
    }

    fn visit_item_struct(&mut self, node: &'ast syn::ItemStruct) {
        if matches!(node.vis, Visibility::Public(_)) {
            for field in &node.fields {
                if matches!(field.vis, Visibility::Public(_)) {
                    self.check_type(&field.ty);
                }
            }
        }
        visit::visit_item_struct(self, node);
    }

    fn visit_item_enum(&mut self, node: &'ast syn::ItemEnum) {
        if matches!(node.vis, Visibility::Public(_)) {
            for variant in &node.variants {
                for field in &variant.fields {
                    self.check_type(&field.ty);
                }
            }
        }
        visit::visit_item_enum(self, node);
    }

    fn visit_item_trait(&mut self, node: &'ast syn::ItemTrait) {
        if matches!(node.vis, Visibility::Public(_)) {
            for item in &node.items {
                if let syn::TraitItem::Fn(method) = item {
                    self.check_signature(&method.sig);
                }
            }
        }
        visit::visit_item_trait(self, node);
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        if node.trait_.is_none() && self.is_local_public_type(&node.self_ty) {
            for item in &node.items {
                if let syn::ImplItem::Fn(method) = item
                    && matches!(method.vis, Visibility::Public(_))
                {
                    self.check_signature(&method.sig);
                }
            }
        }
        visit::visit_item_impl(self, node);
    }
}

/// The `pub` type names declared in `file`, inline modules included.
///
/// An inherent impl resolves its self type against this file-local set: an
/// `impl` living in another file from its `pub` type is not recognised as the
/// edge, and the module chain's visibility is not resolved — `pub` is `pub` at
/// any depth, the same convention the item-name half already follows.
fn public_type_names(file: &syn::File) -> Vec<String> {
    let mut names = Vec::new();
    collect_public_type_names(&file.items, &mut names);
    names
}

fn collect_public_type_names(items: &[Item], out: &mut Vec<String>) {
    for item in items {
        match item {
            Item::Struct(item) if matches!(item.vis, Visibility::Public(_)) => {
                out.push(item.ident.to_string());
            }
            Item::Enum(item) if matches!(item.vis, Visibility::Public(_)) => {
                out.push(item.ident.to_string());
            }
            Item::Union(item) if matches!(item.vis, Visibility::Public(_)) => {
                out.push(item.ident.to_string());
            }
            Item::Type(item) if matches!(item.vis, Visibility::Public(_)) => {
                out.push(item.ident.to_string());
            }
            Item::Mod(item_mod) => {
                if let Some((_, nested)) = &item_mod.content {
                    collect_public_type_names(nested, out);
                }
            }
            _ => {}
        }
    }
}

/// The ident of the item kinds that carry a name across a crate edge.
fn item_ident(item: &Item) -> Option<&syn::Ident> {
    match item {
        Item::Struct(item) => Some(&item.ident),
        Item::Enum(item) => Some(&item.ident),
        Item::Trait(item) => Some(&item.ident),
        Item::TraitAlias(item) => Some(&item.ident),
        Item::Type(item) => Some(&item.ident),
        Item::Union(item) => Some(&item.ident),
        Item::Const(item) => Some(&item.ident),
        Item::Static(item) => Some(&item.ident),
        Item::Fn(item) => Some(&item.sig.ident),
        _ => None,
    }
}

/// Whether the item is part of its crate's public API.
fn is_public(item: &Item) -> bool {
    let vis = match item {
        Item::Struct(item) => &item.vis,
        Item::Enum(item) => &item.vis,
        Item::Trait(item) => &item.vis,
        Item::TraitAlias(item) => &item.vis,
        Item::Type(item) => &item.vis,
        Item::Union(item) => &item.vis,
        Item::Const(item) => &item.vis,
        Item::Static(item) => &item.vis,
        Item::Fn(item) => &item.vis,
        Item::Use(item) => &item.vis,
        _ => return false,
    };
    matches!(vis, Visibility::Public(_))
}

/// The idents `ty` names at its edge: the last segment of every path it is made
/// of, through references and one level of generic arguments.
fn type_idents(ty: &Type) -> Vec<&syn::Ident> {
    let mut idents = Vec::new();
    collect_type_idents(ty, &mut idents);
    idents
}

fn collect_type_idents<'a>(ty: &'a Type, out: &mut Vec<&'a syn::Ident>) {
    match ty {
        Type::Reference(reference) => collect_type_idents(&reference.elem, out),
        Type::Path(path) => {
            if let Some(segment) = path.path.segments.last() {
                out.push(&segment.ident);
                if let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments {
                    for argument in &arguments.args {
                        if let syn::GenericArgument::Type(ty) = argument {
                            collect_type_idents(ty, out);
                        }
                    }
                }
            }
        }
        Type::Paren(paren) => collect_type_idents(&paren.elem, out),
        Type::Slice(slice) => collect_type_idents(&slice.elem, out),
        Type::Array(array) => collect_type_idents(&array.elem, out),
        _ => {}
    }
}

/// Every leaf ident a `use` tree imports.
fn collect_use_leaves(tree: &UseTree, visit: &mut impl FnMut(&syn::Ident)) {
    match tree {
        UseTree::Name(name) => visit(&name.ident),
        UseTree::Rename(rename) => visit(&rename.ident),
        UseTree::Path(path) => collect_use_leaves(&path.tree, visit),
        UseTree::Group(group) => {
            for tree in &group.items {
                collect_use_leaves(tree, visit);
            }
        }
        UseTree::Glob(_) => {}
    }
}

// endregion: --- Support
