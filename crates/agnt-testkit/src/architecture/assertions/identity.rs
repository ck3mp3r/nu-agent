//! ARCH-11: identity reachability and the kernel's display vocabulary.

use syn::{Ident, Item, Type, Visibility};

use crate::architecture::report::Violation;
use crate::architecture::sources::Workspace;

use super::support::{field_line, field_name, labels, walk};

// region:    --- Functions

/// WHEN a struct under `crates/*/src` declares a public field typed as one of
/// the identity newtypes, THEN the identity SHALL be publicly settable and the
/// assertion SHALL report the field.
///
/// The identity contract is what makes the field a violation: an identity
/// derived from the record's own content cannot be replaced from outside, so a
/// public field lets a record exist whose id disagrees with what it describes.
pub fn arch_11a_public_identity_fields(ws: &Workspace) -> Vec<Violation> {
    let mut violations = Vec::new();
    for label in labels(ws) {
        let Some(file) = ws.rust_file(label) else {
            continue;
        };
        for item in walk(&file.items) {
            let Item::Struct(item_struct) = item else {
                continue;
            };
            for (index, field) in item_struct.fields.iter().enumerate() {
                if !matches!(field.vis, Visibility::Public(_)) {
                    continue;
                }
                let Some(identity) = identity_type(&field.ty) else {
                    continue;
                };
                violations.push(Violation {
                    assertion: ASSERTION_IDENTITY,
                    site: format!("{label}:{}", field_line(field)),
                    detail: format!("pub {}: {identity}", field_name(field, index)),
                });
            }
        }
    }
    violations
}

/// WHEN a declaration under `crates/agnt-kernel/src` is named as one of the
/// display types, THEN the kernel SHALL name a rendering and the assertion
/// SHALL report the declaration.
///
/// A doc comment is not a declaration, and a type named outside the kernel is
/// not the kernel's: the check reads parsed items, so neither fires.
pub fn arch_11b_kernel_names_no_display_type(ws: &Workspace) -> Vec<Violation> {
    let mut violations = Vec::new();
    for label in labels(ws).filter(|label| label.starts_with(KERNEL_SOURCES)) {
        let Some(file) = ws.rust_file(label) else {
            continue;
        };
        for item in walk(&file.items) {
            let Some((kind, ident)) = declared_type(item) else {
                continue;
            };
            let name = ident.to_string();
            if !DISPLAY_TYPES.contains(&name.as_str()) {
                continue;
            }
            violations.push(Violation {
                assertion: ASSERTION_DISPLAY,
                site: format!("{label}:{}", ident.span().start().line),
                detail: format!("`{kind} {name}` is a display type in the kernel"),
            });
        }
    }
    violations
}

// endregion: --- Functions

// region:    --- Support

/// The assertion a publicly settable identity field breaks.
const ASSERTION_IDENTITY: &str = "ARCH-11.a";

/// The assertion a display type named in the kernel breaks.
const ASSERTION_DISPLAY: &str = "ARCH-11.b";

/// The kernel's sources, by the label prefix the loader gives them.
const KERNEL_SOURCES: &str = "agnt-kernel/src/";

/// The identity newtypes a field must not expose publicly.
const IDENTITY_TYPES: [&str; 3] = ["ToolCallId", "RequestId", "SessionId"];

/// The names a display type carries.
const DISPLAY_TYPES: [&str; 7] = [
    "Display",
    "Section",
    "DisplayStats",
    "ContentKind",
    "StyleHint",
    "Preview",
    "CallLine",
];

/// The identity newtype `ty` names, when it names one outright.
fn identity_type(ty: &Type) -> Option<&Ident> {
    let Type::Path(path) = ty else {
        return None;
    };
    let ident = &path.path.segments.last()?.ident;
    IDENTITY_TYPES
        .contains(&ident.to_string().as_str())
        .then_some(ident)
}

/// The kind and name a declaration carries, when it declares a type.
fn declared_type(item: &Item) -> Option<(&'static str, &Ident)> {
    match item {
        Item::Struct(item) => Some(("struct", &item.ident)),
        Item::Enum(item) => Some(("enum", &item.ident)),
        Item::Type(item) => Some(("type", &item.ident)),
        Item::Trait(item) => Some(("trait", &item.ident)),
        _ => None,
    }
}

// endregion: --- Support
