//! ARCH-2: the rendering and orchestration vocabularies stay in their own crates.

use crate::architecture::{Workspace, arch_2_vocabulary_purity, assert_matches_baseline};

use super::support::{self, ARCH_2, Result};

// region:    --- Fixtures

/// The two crates the check reads, and the fixture file every case writes.
const RUNTIME: &str = "agnt-runtime";
const RENDER: &str = "agnt-render";
const SOURCE: &str = "rogue.rs";

/// A runtime public item naming a drawer's vocabulary.
const RENDER_TERM_IN_RUNTIME: &str = "pub struct FramePlan;\n";

/// A render public function naming a bus payload family.
const ORCH_TERM_IN_RENDER: &str = "pub fn fold(_record: &SessionRecord) {}\n";

/// The same words as text: a doc comment and a string literal are not names.
const NAMED_IN_TEXT: &str =
    "/// `SessionRecord` rides `stream`.\npub const LABEL: &str = \"FramePlan\";\n";

/// A private item naming a drawer's vocabulary: not the public API.
const PRIVATE_RENDER_TERM: &str = "struct FramePlan;\n";

/// A public field whose type is the drawer's vocabulary.
const FIELD_LEAK: &str = "pub struct Holder {\n    pub plan: FramePlan,\n}\n";

/// A public trait method whose signature names the drawer's vocabulary.
const TRAIT_SIGNATURE_LEAK: &str = "pub trait Drawer {\n    fn draw(&self, plan: FramePlan);\n}\n";

/// A public inherent method whose signature names the drawer's vocabulary.
const INHERENT_SIGNATURE_LEAK: &str = "pub struct Widget;\n\nimpl Widget {\n    pub fn plan(&self) -> FramePlan {\n        todo!()\n    }\n}\n";

/// The same field, private: not the crate edge.
const PRIVATE_FIELD: &str = "pub struct Holder {\n    plan: FramePlan,\n}\n";

/// The same method, private: not the crate edge.
const PRIVATE_METHOD: &str = "pub struct Widget;\n\nimpl Widget {\n    fn plan(&self) -> FramePlan {\n        todo!()\n    }\n}\n";

/// An inherent impl whose self type is not declared in this file: the scan is
/// file-local and says so.
const FOREIGN_SELF_TYPE: &str =
    "impl Widget {\n    pub fn plan(&self) -> FramePlan {\n        todo!()\n    }\n}\n";

// endregion: --- Fixtures

// region:    --- Drivers

/// WHEN the purity scan runs against today's tree, THE violation set SHALL equal
/// the recorded baseline exactly.
#[test]
fn test_purity_arch_2_on_baseline() -> Result<()> {
    // -- Setup & Fixtures
    let ws = Workspace::load()?;
    let loaded = ws
        .sources()
        .iter()
        .any(|(label, _)| label.starts_with("agnt-render/src/"));

    // -- Exec
    let found = arch_2_vocabulary_purity(&ws);

    // -- Check
    assert!(loaded, "the drawer crate must be loaded");
    assert_matches_baseline(ARCH_2, &found, support::baseline(ARCH_2));
    Ok(())
}

/// WHEN the runtime's public API names a drawer's vocabulary, THEN the assertion
/// SHALL report that line.
#[test]
fn test_purity_arch_2_flags_a_render_term_in_the_runtime_api() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-2-runtime-render-term")?;
    support::write_source(&root, RUNTIME, SOURCE, RENDER_TERM_IN_RUNTIME)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_2_vocabulary_purity(&ws);

    // -- Check
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].assertion, ARCH_2);
    assert_eq!(found[0].site, "agnt-runtime/src/rogue.rs:1");
    assert_eq!(found[0].detail, "public API names `FramePlan`");
    Ok(())
}

/// WHEN the drawer's public API names a bus payload family, THEN the assertion
/// SHALL report that line.
#[test]
fn test_purity_arch_2_flags_an_orchestration_term_in_the_render_api() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-2-render-orch-term")?;
    support::write_source(&root, RENDER, SOURCE, ORCH_TERM_IN_RENDER)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_2_vocabulary_purity(&ws);

    // -- Check
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].site, "agnt-render/src/rogue.rs:1");
    assert_eq!(found[0].detail, "public API names `SessionRecord`");
    Ok(())
}

/// WHEN the banned words appear only as text, THEN the assertion SHALL report
/// nothing: the scan reads symbols, never lines.
#[test]
fn test_purity_arch_2_ignores_text() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-2-named-in-text")?;
    support::write_source(&root, RENDER, SOURCE, NAMED_IN_TEXT)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_2_vocabulary_purity(&ws);

    // -- Check
    assert!(found.is_empty(), "{found:#?}");
    Ok(())
}

/// WHEN a private item names a banned term, THEN the assertion SHALL report
/// nothing: the crate edge is the public API.
#[test]
fn test_purity_arch_2_ignores_a_private_item() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-2-private-item")?;
    support::write_source(&root, RUNTIME, SOURCE, PRIVATE_RENDER_TERM)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_2_vocabulary_purity(&ws);

    // -- Check
    assert!(found.is_empty(), "{found:#?}");
    Ok(())
}

/// WHEN a public field's type is the drawer's vocabulary, THEN the assertion
/// SHALL report that field: a struct field is part of the crate edge.
#[test]
fn test_purity_arch_2_flags_a_render_term_in_a_public_field() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-2-public-field")?;
    support::write_source(&root, RUNTIME, SOURCE, FIELD_LEAK)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_2_vocabulary_purity(&ws);

    // -- Check
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].site, "agnt-runtime/src/rogue.rs:2");
    assert_eq!(found[0].detail, "public API names `FramePlan`");
    Ok(())
}

/// WHEN a public trait method's signature names the drawer's vocabulary, THEN
/// the assertion SHALL report that signature.
#[test]
fn test_purity_arch_2_flags_a_render_term_in_a_public_trait_signature() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-2-trait-signature")?;
    support::write_source(&root, RUNTIME, SOURCE, TRAIT_SIGNATURE_LEAK)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_2_vocabulary_purity(&ws);

    // -- Check
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].site, "agnt-runtime/src/rogue.rs:2");
    assert_eq!(found[0].detail, "public API names `FramePlan`");
    Ok(())
}

/// WHEN a public inherent method's signature names the drawer's vocabulary, THEN
/// the assertion SHALL report that signature.
#[test]
fn test_purity_arch_2_flags_a_render_term_in_an_inherent_signature() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-2-inherent-signature")?;
    support::write_source(&root, RUNTIME, SOURCE, INHERENT_SIGNATURE_LEAK)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_2_vocabulary_purity(&ws);

    // -- Check
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].site, "agnt-runtime/src/rogue.rs:4");
    assert_eq!(found[0].detail, "public API names `FramePlan`");
    Ok(())
}

/// WHEN a field is private, THEN the assertion SHALL report nothing: the edge is
/// the public surface.
#[test]
fn test_purity_arch_2_ignores_a_private_field() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-2-private-field")?;
    support::write_source(&root, RUNTIME, SOURCE, PRIVATE_FIELD)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_2_vocabulary_purity(&ws);

    // -- Check
    assert!(found.is_empty(), "{found:#?}");
    Ok(())
}

/// WHEN an inherent method is private, THEN the assertion SHALL report nothing.
#[test]
fn test_purity_arch_2_ignores_a_private_method() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-2-private-method")?;
    support::write_source(&root, RUNTIME, SOURCE, PRIVATE_METHOD)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_2_vocabulary_purity(&ws);

    // -- Check
    assert!(found.is_empty(), "{found:#?}");
    Ok(())
}

/// WHEN an inherent impl's self type is not declared in the file, THEN the
/// assertion SHALL report nothing: the scan resolves the self type file-locally
/// and states that limit.
#[test]
fn test_purity_arch_2_ignores_an_impl_outside_the_declaring_file() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-2-foreign-self-type")?;
    support::write_source(&root, RUNTIME, SOURCE, FOREIGN_SELF_TYPE)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_2_vocabulary_purity(&ws);

    // -- Check
    assert!(found.is_empty(), "{found:#?}");
    Ok(())
}

// endregion: --- Drivers
