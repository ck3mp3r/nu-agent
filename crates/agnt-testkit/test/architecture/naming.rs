//! ARCH-10: no consumer names the aggregate runtime type.
//!
//! The naming checks share this driver: the file-name marker check (`NAMES`),
//! whose subject is a path, and the type-name marker check (`TYPES`), whose
//! subject is a parsed declaration. Neither holds a baseline row — a marked name
//! fails outright.

use crate::architecture::{
    Workspace, arch_10_aggregate_runtime_references, assert_matches_baseline,
    marked_source_filenames, marked_type_names,
};

use super::support::{self, ARCH_10, NAMES, Result, TYPES};

// region:    --- Fixtures

/// The crate every fixture lives in: a consumer crate.
const CRATE: &str = "agnt-cli";

/// A file with no recorded references.
const SOURCE: &str = "rogue.rs";

/// A signature that names the aggregate.
const NAMED_IN_SIGNATURE: &str = "pub fn f(_runtime: &AgentConversationRuntime) {}\n";

/// The same name in a doc comment: a string literal, never a path.
const NAMED_IN_DOC: &str = "/// `AgentConversationRuntime` drives a mode.\npub fn f() {}\n";

/// A different type whose name starts with the aggregate's.
const BUILDER: &str = "pub fn f(_b: AgentRuntimeBuilder) {}\n";

/// Content the filename check never reads: the file name is the subject.
const MARKER_CONTENT: &str = "pub fn f() {}\n";

/// A file name whose stem ends with the banned word.
const MARKED_TRAILING: &str = "rogue_impl.rs";

/// A file name whose stem begins with the banned word.
const MARKED_LEADING: &str = "impl_rogue.rs";

/// A directory whose name carries the banned word, holding a clean file.
const MARKED_DIRECTORY_SOURCE: &str = "impl_rogue/clean.rs";

/// Content for the type-name fixtures: one declaration and nothing else.
const MARKED_STRUCT: &str = "struct RogueImpl;\n";

/// An enum whose leading segment is the banned word.
const MARKED_ENUM: &str = "enum ImplRogue {}\n";

/// A type name that holds the letters but not the segment.
const MARKED_LETTERS_ONLY: &str = "struct Simple;\n";

// endregion: --- Fixtures

// region:    --- Drivers

/// WHEN the census runs against today's tree, THE violation set SHALL equal
/// the recorded baseline exactly.
#[test]
fn test_naming_arch_10_on_baseline() -> Result<()> {
    // -- Setup & Fixtures
    let ws = Workspace::load()?;
    let loaded = ws
        .sources()
        .iter()
        .any(|(label, _)| label == "agnt-cli/src/mode/tui.rs");

    // -- Exec
    let found = arch_10_aggregate_runtime_references(&ws);

    // -- Check
    assert!(loaded, "the consumer crates must be loaded");
    assert_matches_baseline(ARCH_10, &found, support::baseline(ARCH_10));
    Ok(())
}

/// WHEN a consumer's signature names the aggregate, THEN the assertion SHALL
/// report that line.
#[test]
fn test_naming_arch_10_flags_a_named_signature() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-10-named-signature")?;
    support::write_source(&root, CRATE, SOURCE, NAMED_IN_SIGNATURE)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_10_aggregate_runtime_references(&ws);

    // -- Check
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].assertion, ARCH_10);
    assert_eq!(found[0].site, "agnt-cli/src/rogue.rs:1");
    assert_eq!(found[0].detail, "AgentConversationRuntime");
    Ok(())
}

/// WHEN a doc comment names the aggregate, THEN the assertion SHALL report
/// nothing: a name in text is not a reference.
#[test]
fn test_naming_arch_10_ignores_a_doc_comment() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-10-named-in-doc")?;
    support::write_source(&root, CRATE, SOURCE, NAMED_IN_DOC)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_10_aggregate_runtime_references(&ws);

    // -- Check
    assert!(found.is_empty(), "{found:#?}");
    Ok(())
}

/// WHEN a different type carries the aggregate's name as a prefix, THEN the
/// assertion SHALL report nothing: the match is the last segment, exactly.
#[test]
fn test_naming_arch_10_ignores_a_prefixed_type() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-10-prefixed-type")?;
    support::write_source(&root, CRATE, SOURCE, BUILDER)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_10_aggregate_runtime_references(&ws);

    // -- Check
    assert!(found.is_empty(), "{found:#?}");
    Ok(())
}

/// WHEN the tree holds no marked file name, THEN the filename check SHALL report
/// nothing: the rename in this task is what makes the tree clean.
#[test]
fn test_naming_marked_source_filenames_pass_on_the_tree() -> Result<()> {
    // -- Setup & Fixtures
    let ws = Workspace::load()?;

    // -- Exec
    let found = marked_source_filenames(&ws)?;

    // -- Check
    assert!(
        !ws.sources().is_empty(),
        "an unloaded workspace would pass this check"
    );
    assert!(found.is_empty(), "{found:#?}");
    Ok(())
}

/// WHEN a source file's stem ends with `_impl`, THEN the filename check SHALL
/// fail and name the path.
#[test]
fn test_naming_marked_source_filenames_flags_a_trailing_marker() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("names-trailing-marker")?;
    support::write_source(&root, CRATE, MARKED_TRAILING, MARKER_CONTENT)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = marked_source_filenames(&ws)?;

    // -- Check
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].assertion, NAMES);
    assert_eq!(found[0].site, "agnt-cli/src/rogue_impl.rs");
    assert_eq!(
        found[0].detail,
        "`rogue_impl.rs` carries the `impl` implementation marker"
    );
    Ok(())
}

/// WHEN a source file's stem begins with `impl_`, THEN the filename check SHALL
/// fail and name the path.
#[test]
fn test_naming_marked_source_filenames_flags_a_leading_marker() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("names-leading-marker")?;
    support::write_source(&root, CRATE, MARKED_LEADING, MARKER_CONTENT)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = marked_source_filenames(&ws)?;

    // -- Check
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].site, "agnt-cli/src/impl_rogue.rs");
    Ok(())
}

/// WHEN every loaded file name is clean, THEN the filename check SHALL report
/// nothing — including a directory that carries the word, since the rule is about
/// file names.
#[test]
fn test_naming_marked_source_filenames_ignores_clean_names() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("names-clean")?;
    support::write_source(&root, CRATE, SOURCE, MARKER_CONTENT)?;
    support::write_source(&root, CRATE, MARKED_DIRECTORY_SOURCE, MARKER_CONTENT)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = marked_source_filenames(&ws)?;

    // -- Check
    assert!(found.is_empty(), "{found:#?}");
    Ok(())
}

/// WHEN a crate's test tree holds a marked file name, THEN the filename check
/// SHALL fail with its path: the measured set is every `.rs` path under
/// `crates/`, not only the sources the census parses.
#[test]
fn test_naming_marked_source_filenames_flags_a_marked_test_file() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("names-test-tree-marker")?;
    support::write_test_source(&root, CRATE, MARKED_TRAILING, MARKER_CONTENT)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = marked_source_filenames(&ws)?;

    // -- Check
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].site, "agnt-cli/test/rogue_impl.rs");
    assert!(
        ws.sources().is_empty(),
        "the test tree must stay out of the parsed source set the census reads"
    );
    Ok(())
}

/// WHEN the tree declares no marked type name, THEN the type-name check SHALL
/// report nothing: the rename in this task is what makes the tree clean.
#[test]
fn test_naming_marked_type_names_pass_on_the_tree() -> Result<()> {
    // -- Setup & Fixtures
    let ws = Workspace::load()?;

    // -- Exec
    let found = marked_type_names(&ws);

    // -- Check
    assert!(
        !ws.sources().is_empty(),
        "an unloaded workspace would pass this check"
    );
    assert!(found.is_empty(), "{found:#?}");
    Ok(())
}

/// WHEN a struct is declared with the banned segment in its name, THEN the
/// type-name check SHALL fail and name the declaration line.
#[test]
fn test_naming_marked_type_names_flags_a_marked_struct() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("types-marked-struct")?;
    support::write_source(&root, CRATE, SOURCE, MARKED_STRUCT)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = marked_type_names(&ws);

    // -- Check
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].assertion, TYPES);
    assert_eq!(found[0].site, "agnt-cli/src/rogue.rs:1");
    assert_eq!(
        found[0].detail,
        "`RogueImpl` carries the `Impl` name segment"
    );
    Ok(())
}

/// WHEN an enum's leading segment is the banned word, THEN the type-name check
/// SHALL fail: the rule is the segment, not the suffix.
#[test]
fn test_naming_marked_type_names_flags_a_leading_segment() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("types-marked-enum")?;
    support::write_source(&root, CRATE, SOURCE, MARKED_ENUM)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = marked_type_names(&ws);

    // -- Check
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].site, "agnt-cli/src/rogue.rs:1");
    Ok(())
}

/// WHEN a type name holds the letters but not the segment, THEN the type-name
/// check SHALL report nothing: `Simple` is one segment and never matches.
#[test]
fn test_naming_marked_type_names_ignores_the_letters_alone() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("types-letters-only")?;
    support::write_source(&root, CRATE, SOURCE, MARKED_LETTERS_ONLY)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = marked_type_names(&ws);

    // -- Check
    assert!(found.is_empty(), "{found:#?}");
    Ok(())
}

// endregion: --- Drivers
