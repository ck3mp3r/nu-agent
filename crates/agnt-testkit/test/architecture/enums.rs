//! ARCH-4: duplicate `pub enum` names across the workspace.

use crate::architecture::{Workspace, arch_4_duplicate_enum_names, assert_matches_baseline};

use super::support::{self, ARCH_4, Result};

// region:    --- Fixtures

/// A source every workspace carries, and the enum it declares.
const KERNEL_SOURCE: &str = "agnt-kernel/src/records.rs";

/// The duplicate name every rogue fixture uses.
const DUPLICATE: &str = "ToolErrorKind";

/// One `pub enum` of [`DUPLICATE`].
const PUBLIC_ENUM: &str = "pub enum ToolErrorKind { A }\n";

/// A second `pub enum` of the same name, in another crate.
const OTHER_PUBLIC_ENUM: &str = "pub enum ToolErrorKind { B }\n";

/// The same name, visible only inside its crate.
const CRATE_ENUM: &str = "pub(crate) enum ToolErrorKind { B }\n";

/// A private enum nested inside a function body.
const NESTED_ENUM: &str = "pub fn holder() {\n    enum Nested { A }\n}\n";

/// A type whose doc comment spells a declaration the module does not make.
const DOC_COMMENT: &str = concat!(
    "/// The kernel's `pub enum ToolErrorKind` is the only one.\n",
    "pub struct Holder;\n",
);

// endregion: --- Fixtures

// region:    --- Drivers

/// WHEN the census runs against today's tree, THE violation set SHALL equal the
/// recorded baseline exactly.
#[test]
fn test_enums_arch_4_duplicate_enum_names_pass_on_baseline() -> Result<()> {
    // -- Setup & Fixtures
    let ws = Workspace::load()?;
    let loaded: Vec<&str> = ws
        .sources()
        .iter()
        .map(|(label, _)| label.as_str())
        .collect();
    let kernel = ws
        .rust_file(KERNEL_SOURCE)
        .ok_or("the kernel records must be parsed")?;

    // -- Exec
    let found = arch_4_duplicate_enum_names(&ws);

    // -- Check
    assert!(
        loaded.contains(&KERNEL_SOURCE),
        "the enum declarations must be loaded"
    );
    assert!(
        kernel.items.iter().any(|item| matches!(
            item,
            syn::Item::Enum(item_enum) if item_enum.ident == "ToolErrorKind"
        )),
        "the baseline's kernel declaration must be seen by the parser"
    );
    assert_matches_baseline(ARCH_4, &found, support::baseline(ARCH_4));
    Ok(())
}

/// WHEN two crates each declare a `pub enum` of one name, THEN the census SHALL
/// report both declarations at their lines.
#[test]
fn test_enums_arch_4_duplicate_enum_names_flags_two_crates() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-4-two-crates")?;
    support::write_source(&root, "agnt-kernel", "records.rs", PUBLIC_ENUM)?;
    support::write_source(&root, "agnt-runtime", "types.rs", OTHER_PUBLIC_ENUM)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_4_duplicate_enum_names(&ws);

    // -- Check
    let mut sites: Vec<&str> = found
        .iter()
        .map(|violation| violation.site.as_str())
        .collect();
    sites.sort_unstable();
    assert_eq!(
        sites,
        [
            "agnt-kernel/src/records.rs:1",
            "agnt-runtime/src/types.rs:1"
        ]
    );
    assert!(
        found
            .iter()
            .all(|violation| violation.detail.contains(DUPLICATE)),
        "{found:#?}"
    );
    Ok(())
}

/// WHEN one crate declares a `pub enum` of a name no other declaration
/// carries, THEN the census SHALL report nothing.
#[test]
fn test_enums_arch_4_duplicate_enum_names_ignores_a_single_declaration() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-4-single-declaration")?;
    support::write_source(&root, "agnt-kernel", "records.rs", PUBLIC_ENUM)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_4_duplicate_enum_names(&ws);

    // -- Check
    assert!(found.is_empty(), "{found:#?}");
    Ok(())
}

/// WHEN the second declaration of a name is visible only inside its crate or
/// nested inside a function body, THEN the census SHALL report nothing: it
/// counts public module-level declarations.
#[test]
fn test_enums_arch_4_duplicate_enum_names_ignores_non_public_and_nested() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-4-visibility")?;
    support::write_source(&root, "agnt-kernel", "records.rs", PUBLIC_ENUM)?;
    support::write_source(&root, "agnt-runtime", "types.rs", CRATE_ENUM)?;
    support::write_source(&root, "agnt-runtime", "nested.rs", NESTED_ENUM)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_4_duplicate_enum_names(&ws);

    // -- Check
    assert!(found.is_empty(), "{found:#?}");
    Ok(())
}

/// WHEN a doc comment spells a declaration the module does not make, THEN the
/// census SHALL report nothing: a comment is not a declaration.
#[test]
fn test_enums_arch_4_duplicate_enum_names_ignores_a_doc_comment() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-4-doc-comment")?;
    support::write_source(&root, "agnt-kernel", "records.rs", PUBLIC_ENUM)?;
    support::write_source(&root, "agnt-runtime", "types.rs", DOC_COMMENT)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_4_duplicate_enum_names(&ws);

    // -- Check
    assert!(found.is_empty(), "{found:#?}");
    Ok(())
}

// endregion: --- Drivers
