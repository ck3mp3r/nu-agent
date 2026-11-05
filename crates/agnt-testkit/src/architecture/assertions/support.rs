//! The AST helpers the declaration checks share.

use syn::spanned::Spanned;
use syn::{Field, Item};

use crate::architecture::sources::Workspace;

// region:    --- Functions

/// Every loaded source label, in the loader's order.
pub(super) fn labels(ws: &Workspace) -> impl Iterator<Item = &str> {
    ws.sources().iter().map(|(label, _)| label.as_str())
}

/// Every item in `items`, with the items of inline modules included.
pub(super) fn walk(items: &[Item]) -> Vec<&Item> {
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

/// The field's name, or its position when it is unnamed.
pub(super) fn field_name(field: &Field, index: usize) -> String {
    field
        .ident
        .as_ref()
        .map_or_else(|| index.to_string(), |ident| ident.to_string())
}

/// The line the field declares, not the line its attributes start on.
pub(super) fn field_line(field: &Field) -> usize {
    let span = field
        .ident
        .as_ref()
        .map_or_else(|| field.ty.span(), |ident| ident.span());
    span.start().line
}

// endregion: --- Functions
