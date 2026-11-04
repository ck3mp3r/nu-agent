//! ARCH-11: identity reachability and the kernel's display vocabulary.

use syn::Item;

use crate::architecture::{
    Workspace, arch_11a_public_identity_fields, arch_11b_kernel_names_no_display_type,
    assert_matches_baseline,
};

use super::support::{self, ARCH_11A, ARCH_11B, Result};

// region:    --- Fixtures

/// The crate the display assertion pins.
const KERNEL: &str = "agnt-kernel";

/// The source holding the tree's identity declarations.
const IDENTITY_SOURCE: &str = "agnt-kernel/src/records.rs";

/// The source holding the kernel's kind declarations.
const KIND_SOURCE: &str = "agnt-kernel/src/builtin.rs";

/// A record whose identity is publicly settable, behind a doc comment that must
/// not become the reported line.
const ROGUE_PUBLIC_IDENTITY: &str = concat!(
    "pub struct Rogue {\n",
    "    /// Derived by `ToolCallId::new` and carried.\n",
    "    pub id: ToolCallId,\n",
    "}\n",
);

/// The same field with no `pub`.
const PRIVATE_IDENTITY: &str = concat!(
    "pub struct Rogue {\n",
    "    /// Derived by `ToolCallId::new` and carried.\n",
    "    id: ToolCallId,\n",
    "}\n",
);

/// A display type declared in the kernel.
const ROGUE_DISPLAY_TYPE: &str = "pub struct Display;\n";

/// A permitted kernel type carrying a doc comment that names a display type.
const DISPLAY_IN_A_DOC_COMMENT: &str = concat!(
    "/// The text a consumer renders. A `Display` block is the drawer's, not this\n",
    "/// record's.\n",
    "pub struct NoticeRecord {\n",
    "    pub text: String,\n",
    "}\n",
);

// endregion: --- Fixtures

// region:    --- Drivers

/// WHEN the identity assertion runs against today's tree, THE violation set
/// SHALL equal the recorded baseline exactly.
#[test]
fn test_identity_arch_11a_public_identity_fields_pass_on_baseline() -> Result<()> {
    // -- Setup & Fixtures
    let ws = Workspace::load()?;

    // -- Exec
    let found = arch_11a_public_identity_fields(&ws);

    // -- Check
    assert!(
        loaded(&ws, IDENTITY_SOURCE),
        "the identity declarations must be loaded"
    );
    assert!(
        declares(&ws, IDENTITY_SOURCE, "ToolCall"),
        "the record holding the baseline field must be parsed"
    );
    assert_matches_baseline(ARCH_11A, &found, support::baseline(ARCH_11A));
    Ok(())
}

/// WHEN the display assertion runs against today's tree, THE violation set
/// SHALL be empty and SHALL equal the recorded baseline.
#[test]
fn test_identity_arch_11b_kernel_names_no_display_type_pass_on_empty_baseline() -> Result<()> {
    // -- Setup & Fixtures
    let ws = Workspace::load()?;

    // -- Exec
    let found = arch_11b_kernel_names_no_display_type(&ws);

    // -- Check
    assert!(
        loaded(&ws, IDENTITY_SOURCE) && loaded(&ws, KIND_SOURCE),
        "the kernel's declarations must be loaded"
    );
    assert!(
        declares(&ws, IDENTITY_SOURCE, "SessionRecord"),
        "the kernel's records must be parsed, or an empty set proves nothing"
    );
    assert!(found.is_empty(), "{found:#?}");
    assert_matches_baseline(ARCH_11B, &found, support::baseline(ARCH_11B));
    Ok(())
}

/// WHEN a struct declares a public field typed as an identity newtype, THEN the
/// assertion SHALL report the line the field declares.
#[test]
fn test_identity_arch_11a_public_identity_fields_flags_a_public_identity() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-11a-public-identity")?;
    support::write_source(&root, KERNEL, "records.rs", ROGUE_PUBLIC_IDENTITY)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_11a_public_identity_fields(&ws);

    // -- Check
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].assertion, ARCH_11A);
    assert_eq!(found[0].site, "agnt-kernel/src/records.rs:3");
    assert!(found[0].detail.contains("ToolCallId"), "{:#?}", found[0]);
    Ok(())
}

/// WHEN the same field carries no `pub`, THEN the assertion SHALL report
/// nothing: a private field cannot be replaced from outside.
#[test]
fn test_identity_arch_11a_public_identity_fields_ignores_a_private_identity() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-11a-private-identity")?;
    support::write_source(&root, KERNEL, "records.rs", PRIVATE_IDENTITY)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_11a_public_identity_fields(&ws);

    // -- Check
    assert!(found.is_empty(), "{found:#?}");
    Ok(())
}

/// WHEN the kernel declares a display type, THEN the assertion SHALL report the
/// declaration at its line.
#[test]
fn test_identity_arch_11b_kernel_names_no_display_type_flags_a_display_type() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-11b-display-type")?;
    support::write_source(&root, KERNEL, "render.rs", ROGUE_DISPLAY_TYPE)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_11b_kernel_names_no_display_type(&ws);

    // -- Check
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].assertion, ARCH_11B);
    assert_eq!(found[0].site, "agnt-kernel/src/render.rs:1");
    assert!(found[0].detail.contains("Display"), "{:#?}", found[0]);
    Ok(())
}

/// WHEN a permitted kernel type carries a doc comment naming a display type,
/// THEN the assertion SHALL report nothing: a comment is not a declaration.
#[test]
fn test_identity_arch_11b_kernel_names_no_display_type_ignores_a_doc_comment() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-11b-doc-comment")?;
    support::write_source(&root, KERNEL, "records.rs", DISPLAY_IN_A_DOC_COMMENT)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_11b_kernel_names_no_display_type(&ws);

    // -- Check
    assert!(found.is_empty(), "{found:#?}");
    Ok(())
}

// endregion: --- Drivers

// region:    --- Test Support

/// Whether the workspace holds a source at `path`.
fn loaded(ws: &Workspace, path: &str) -> bool {
    ws.sources().iter().any(|(label, _)| label == path)
}

/// Whether `path` declares a top-level type named `name`.
fn declares(ws: &Workspace, path: &str, name: &str) -> bool {
    let Some(file) = ws.rust_file(path) else {
        return false;
    };
    file.items.iter().any(|item| match item {
        Item::Struct(item) => item.ident == name,
        Item::Enum(item) => item.ident == name,
        Item::Type(item) => item.ident == name,
        Item::Trait(item) => item.ident == name,
        _ => false,
    })
}

// endregion: --- Test Support
