//! ARCH-8: no function outside the composition root takes or hands out the whole `Bus`.

use syn::spanned::Spanned;
use syn::{FnArg, ImplItem, Item, ReturnType, TraitItem, Type};

use crate::architecture::report::Violation;
use crate::architecture::sources::Workspace;

// region:    --- Functions

/// WHEN a function parameter, a function return type or a struct field names
/// `Bus` — by reference, owned, or wrapped in `Arc`/`Box` — THEN the component
/// SHALL take the whole bus instead of the channel it uses and the assertion
/// SHALL report the site.
///
/// Every spelling of the type is one path: `&Bus`, `&crate::bus::Bus`,
/// `&agnt_runtime::bus::Bus` and `&'a Bus` all end in the same segment, and an
/// owned `Bus`, `Arc<Bus>` or `Box<Bus>` is the same type held rather than
/// borrowed. The match reads the parsed type, so the name inside a comment or a
/// string literal is not a site.
///
/// The aggregate's own field and the private parts view it is assembled from are
/// the composition root: those two files may hold the bus.
pub fn arch_8_whole_bus(ws: &Workspace) -> Vec<Violation> {
    let mut violations = Vec::new();
    for (label, _) in ws.sources() {
        if ROOT_FILES.contains(&label.as_str()) {
            continue;
        }
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

/// The files that may hold the bus: the aggregate's own field and the private
/// parts view it is assembled from.
const ROOT_FILES: [&str; 2] = [
    "agnt-runtime/src/conversation/runtime.rs",
    "agnt-runtime/src/conversation/runtime_parts.rs",
];

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
                    if let Some(shape) = bus_shape(&field.ty) {
                        out.push(site(label, field.ty.span(), FIELD, shape));
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
        if let Some(shape) = bus_shape(&pat_type.ty) {
            out.push(site(label, pat_type.ty.span(), PARAMETER, shape));
        }
    }
    if let ReturnType::Type(_, ty) = &sig.output
        && let Some(shape) = bus_shape(ty)
    {
        out.push(site(label, ty.span(), RETURN, shape));
    }
}

/// The violation `kind` reports at `span` in `label`, for the bus `shape` named.
fn site(
    label: &str,
    span: proc_macro2::Span,
    kind: &'static str,
    shape: &'static str,
) -> Violation {
    Violation {
        assertion: ASSERTION,
        site: format!("{label}:{}", span.start().line),
        detail: format!("`{shape}` is a {kind}"),
    }
}

/// How a type holds the bus: borrowed, owned, or wrapped, when it does.
fn bus_shape(ty: &Type) -> Option<&'static str> {
    if let Type::Reference(reference) = ty {
        return path_names_bus(&reference.elem).then_some("&Bus");
    }
    if path_names_bus(ty) {
        return Some("Bus");
    }
    let Type::Path(path) = ty else {
        return None;
    };
    let segment = path.path.segments.last()?;
    let wrapper = match segment.ident.to_string().as_str() {
        "Arc" => "Arc<Bus>",
        "Box" => "Box<Bus>",
        _ => return None,
    };
    path_names_bus(single_type_argument(segment)?).then_some(wrapper)
}

/// The one type argument a wrapper segment carries.
fn single_type_argument(segment: &syn::PathSegment) -> Option<&Type> {
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return None;
    };
    arguments.args.iter().find_map(|argument| match argument {
        syn::GenericArgument::Type(ty) => Some(ty),
        _ => None,
    })
}

/// Whether `ty` is a path whose last segment is [`BUS`].
fn path_names_bus(ty: &Type) -> bool {
    let Type::Path(path) = ty else {
        return false;
    };
    path.path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == BUS)
}

// endregion: --- Support
