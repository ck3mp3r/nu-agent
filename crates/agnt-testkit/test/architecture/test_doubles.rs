//! ARCH-5: no test double reaches production source.

use crate::architecture::{Workspace, arch_5_rig_test_doubles, assert_matches_baseline};

use super::support::{self, ARCH_5, Result};

// region:    --- Fixtures

/// The crate every rogue fixture lives in.
const CRATE: &str = "agnt-runtime";

/// The source every rogue fixture writes.
const SOURCE: &str = "mod.rs";

/// The source the sanctioned allowance lives in: the census must see it parsed.
const ALLOWANCE_SOURCE: &str = "agnt-runtime/src/conversation/providers.rs";

/// A test double re-exported through a production API.
const RE_EXPORTED_DOUBLE: &str = "pub use rig::test_utils::MockCompletionModel;\n";

/// A test double named in type position, the shape a provider enum carries.
const TYPED_DOUBLE: &str = concat!(
    "pub enum Client {\n",
    "    Mock(rig::test_utils::MockCompletionModel),\n",
    "}\n",
);

/// A local `test_utils` module, and a rig path that is not a test double.
const LOCAL_MODULE_AND_PROVIDER: &str = concat!(
    "pub mod test_utils {\n",
    "    pub struct MockCompletionModel;\n",
    "}\n",
    "pub use test_utils::MockCompletionModel;\n",
    "pub use rig::providers::openai::OpenAI;\n",
);

// endregion: --- Fixtures

// region:    --- Drivers

/// WHEN the assertion runs against today's tree, THE violation set SHALL equal
/// the recorded baseline exactly.
#[test]
fn test_test_doubles_arch_5_rig_test_doubles_pass_on_baseline() -> Result<()> {
    // -- Setup & Fixtures
    let ws = Workspace::load()?;
    let allowance = ws
        .rust_file(ALLOWANCE_SOURCE)
        .ok_or("the allowance's source must be parsed")?;

    // -- Exec
    let found = arch_5_rig_test_doubles(&ws);

    // -- Check
    assert!(
        !allowance.items.is_empty(),
        "the allowance's source must parse into items, or an empty set proves nothing"
    );
    assert_matches_baseline(ARCH_5, &found, support::baseline(ARCH_5));
    Ok(())
}

/// WHEN a production source re-exports a test double, THEN the assertion SHALL
/// report the use tree's line.
#[test]
fn test_test_doubles_arch_5_rig_test_doubles_flags_a_use_tree() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-5-use-tree")?;
    support::write_source(&root, CRATE, SOURCE, RE_EXPORTED_DOUBLE)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_5_rig_test_doubles(&ws);

    // -- Check
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].assertion, ARCH_5);
    assert_eq!(found[0].site, "agnt-runtime/src/mod.rs:1");
    assert_eq!(
        found[0].to_string(),
        "ARCH-5 agnt-runtime/src/mod.rs:1: `rig::test_utils::MockCompletionModel` is a rig test double"
    );
    Ok(())
}

/// WHEN a production type names a test double, THEN the assertion SHALL report
/// the path's line: a payload is a reference, not only a re-export.
#[test]
fn test_test_doubles_arch_5_rig_test_doubles_flags_a_type_path() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-5-type-path")?;
    support::write_source(&root, CRATE, SOURCE, TYPED_DOUBLE)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_5_rig_test_doubles(&ws);

    // -- Check
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].site, "agnt-runtime/src/mod.rs:2");
    Ok(())
}

/// WHEN a source names a local `test_utils` module or a rig provider, THEN the
/// assertion SHALL report nothing: only `rig::test_utils` is a test double.
#[test]
fn test_test_doubles_arch_5_rig_test_doubles_ignores_a_local_module() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-5-local-module")?;
    support::write_source(&root, CRATE, SOURCE, LOCAL_MODULE_AND_PROVIDER)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_5_rig_test_doubles(&ws);

    // -- Check
    assert!(found.is_empty(), "{found:#?}");
    Ok(())
}

// endregion: --- Drivers
