//! ARCH-8: no function outside the composition root takes or hands out the whole `Bus`.

use syn::spanned::Spanned;
use syn::{FnArg, ImplItem, Item, ReturnType, TraitItem, Type};

use crate::architecture::report::Violation;
use crate::architecture::sources::Workspace;

// region:    --- Functions

/// WHEN a function parameter, a function return type or a struct field names
/// `Bus` through a reference, THEN the component SHALL take the whole bus
/// instead of the channel it uses and the assertion SHALL report the site.
///
/// Every spelling of the type is one path: `&Bus`, `&crate::bus::Bus`,
/// `&agnt_runtime::bus::Bus` and `&'a Bus` all end in the same segment. The match
/// reads the parsed type, so the name inside a comment or a string literal is
/// not a site.
pub fn arch_8_whole_bus(ws: &Workspace) -> Vec<Violation> {
    let mut violations = Vec::new();
    for (label, _) in ws.sources() {
        let Some(file) = ws.rust_file(label) else {
            continue;
        };
        collect(&file.items, label, &mut violations);
    }
    violations
}

// endregion: --- Functions

// region:    --- Support

/// The assertion this module reports.
const ASSERTION: &str = "ARCH-8";

/// The type this assertion pins.
const BUS: &str = "Bus";

/// The three places the type can be handed in or out.
const PARAMETER: &str = "parameter";
const RETURN: &str = "return type";
const FIELD: &str = "struct field";

/// Every site in `items`, with the items of inline modules included.
fn collect(items: &[Item], label: &str, out: &mut Vec<Violation>) {
    for item in items {
        match item {
            Item::Fn(item_fn) => collect_signature(&item_fn.sig, label, out),
            Item::Impl(item_impl) => {
                for member in &item_impl.items {
                    if let ImplItem::Fn(item_fn) = member {
                        collect_signature(&item_fn.sig, label, out);
                    }
                }
            }
            Item::Trait(item_trait) => {
                for member in &item_trait.items {
                    if let TraitItem::Fn(item_fn) = member {
                        collect_signature(&item_fn.sig, label, out);
                    }
                }
            }
            Item::Struct(item_struct) => {
                for field in &item_struct.fields {
                    if names_bus(&field.ty) {
                        out.push(site(label, field.ty.span(), FIELD));
                    }
                }
            }
            Item::Mod(item_mod) => {
                if let Some((_, nested)) = &item_mod.content {
                    collect(nested, label, out);
                }
            }
            _ => {}
        }
    }
}

/// Every site a signature carries.
fn collect_signature(sig: &syn::Signature, label: &str, out: &mut Vec<Violation>) {
    for input in &sig.inputs {
        let FnArg::Typed(pat_type) = input else {
            continue;
        };
        if names_bus(&pat_type.ty) {
            out.push(site(label, pat_type.ty.span(), PARAMETER));
        }
    }
    if let ReturnType::Type(_, ty) = &sig.output
        && names_bus(ty)
    {
        out.push(site(label, ty.span(), RETURN));
    }
}

/// The violation `kind` reports at `span` in `label`.
fn site(label: &str, span: proc_macro2::Span, kind: &'static str) -> Violation {
    Violation {
        assertion: ASSERTION,
        site: format!("{label}:{}", span.start().line),
        detail: format!("`&{BUS}` is a {kind}"),
    }
}

/// Whether `ty` is a reference whose innermost path segment is [`BUS`].
fn names_bus(ty: &Type) -> bool {
    let Type::Reference(reference) = ty else {
        return false;
    };
    let Type::Path(path) = reference.elem.as_ref() else {
        return false;
    };
    path.path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == BUS)
}

// endregion: --- Support
