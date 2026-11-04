//! ARCH-8: the whole `Bus` is handed to no component.

use crate::architecture::{Workspace, arch_8_whole_bus, assert_matches_baseline};

use super::support::{self, ARCH_8, Result};

// region:    --- Fixtures

/// The crate every rogue fixture lives in.
const CRATE: &str = "agnt-core";

/// The source every rogue fixture writes.
const SOURCE: &str = "mod.rs";

/// The sites that prove the parsed shape is seen before a match is trusted.
const RETURN_SOURCE: &str = "agnt-acp/src/session.rs";
const FIELD_SOURCE: &str = "agnt-core/src/orchestrator/stages/context.rs";

/// The unqualified spelling.
const PLAIN_PARAMETER: &str = "pub fn run(bus: &Bus) {}\n";

/// The spelling a crate path carries.
const QUALIFIED_PARAMETER: &str = "pub fn run(bus: &crate::bus::Bus) {}\n";

/// The spelling an explicit lifetime carries.
const LIFETIME_PARAMETER: &str = "pub fn run<'a>(bus: &'a Bus) {}\n";

/// A getter that hands the whole bus out.
const RETURNED_BUS: &str = concat!(
    "pub struct Holder;\n",
    "impl Holder {\n",
    "    pub fn bus(&self) -> &Bus {\n",
    "        unimplemented!()\n",
    "    }\n",
    "}\n",
);

/// A struct that keeps the whole bus.
const STORED_BUS: &str = concat!(
    "pub struct Holder<'a> {\n",
    "    pub bus: &'a Bus,\n",
    "}\n",
);

/// The name in a comment, in a doc comment and in a string literal.
const NAMED_IN_TEXT: &str = concat!(
    "// fn f(bus: &Bus)\n",
    "pub const NOTE: &str = \"fn f(bus: &Bus)\";\n",
    "/// fn f(bus: &Bus)\n",
    "pub fn safe(stream: &Stream) {}\n",
);

/// References to the channels a component is supposed to take.
const OTHER_REFERENCES: &str = concat!(
    "pub fn f(stream: &Stream) {}\n",
    "pub fn g(cancel: &CancelTx) {}\n",
);

// endregion: --- Fixtures

// region:    --- Drivers

/// WHEN the assertion runs against today's tree, THE violation set SHALL equal
/// the recorded baseline exactly.
#[test]
fn test_signatures_arch_8_whole_bus_pass_on_baseline() -> Result<()> {
    // -- Setup & Fixtures
    let ws = Workspace::load()?;
    let loaded = |path: &str| ws.sources().iter().any(|(label, _)| label == path);
    let returned = ws
        .rust_file(RETURN_SOURCE)
        .ok_or("the getter's source must be parsed")?;
    let stored = ws
        .rust_file(FIELD_SOURCE)
        .ok_or("the field's source must be parsed")?;

    // -- Exec
    let found = arch_8_whole_bus(&ws);

    // -- Check
    assert!(
        loaded(RETURN_SOURCE) && loaded(FIELD_SOURCE),
        "the return and field sites must be loaded"
    );
    assert!(
        !returned.items.is_empty() && !stored.items.is_empty(),
        "both sources must parse into items, or an empty set proves nothing"
    );
    assert_matches_baseline(ARCH_8, &found, support::baseline(ARCH_8));
    Ok(())
}

/// WHEN a parameter is written `bus: &Bus`, THEN the assertion SHALL report its
/// line.
#[test]
fn test_signatures_arch_8_whole_bus_flags_a_plain_reference() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-8-plain-reference")?;
    support::write_source(&root, CRATE, SOURCE, PLAIN_PARAMETER)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_8_whole_bus(&ws);

    // -- Check
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].assertion, ARCH_8);
    assert_eq!(found[0].site, "agnt-core/src/mod.rs:1");
    Ok(())
}

/// WHEN a parameter is written `bus: &crate::bus::Bus`, THEN the assertion
/// SHALL report its line: the qualified path ends in the same segment.
#[test]
fn test_signatures_arch_8_whole_bus_flags_a_qualified_reference() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-8-qualified-reference")?;
    support::write_source(&root, CRATE, SOURCE, QUALIFIED_PARAMETER)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_8_whole_bus(&ws);

    // -- Check
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].site, "agnt-core/src/mod.rs:1");
    Ok(())
}

/// WHEN a parameter is written `bus: &'a Bus`, THEN the assertion SHALL report
/// its line: a lifetime does not hide the type.
#[test]
fn test_signatures_arch_8_whole_bus_flags_a_lifetime_reference() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-8-lifetime-reference")?;
    support::write_source(&root, CRATE, SOURCE, LIFETIME_PARAMETER)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_8_whole_bus(&ws);

    // -- Check
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].site, "agnt-core/src/mod.rs:1");
    Ok(())
}

/// WHEN a method returns `&Bus`, THEN the assertion SHALL report its line.
#[test]
fn test_signatures_arch_8_whole_bus_flags_a_returned_bus() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-8-returned-bus")?;
    support::write_source(&root, CRATE, SOURCE, RETURNED_BUS)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_8_whole_bus(&ws);

    // -- Check
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].site, "agnt-core/src/mod.rs:3");
    Ok(())
}

/// WHEN a struct field is written `pub bus: &'a Bus`, THEN the assertion SHALL
/// report its line.
#[test]
fn test_signatures_arch_8_whole_bus_flags_a_stored_bus() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-8-stored-bus")?;
    support::write_source(&root, CRATE, SOURCE, STORED_BUS)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_8_whole_bus(&ws);

    // -- Check
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].site, "agnt-core/src/mod.rs:2");
    Ok(())
}

/// WHEN `&Bus` appears only in a comment, a doc comment or a string literal,
/// THEN the assertion SHALL report nothing: a name in text is not a signature.
#[test]
fn test_signatures_arch_8_whole_bus_ignores_the_name_in_text() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-8-named-in-text")?;
    support::write_source(&root, CRATE, SOURCE, NAMED_IN_TEXT)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_8_whole_bus(&ws);

    // -- Check
    assert!(found.is_empty(), "{found:#?}");
    Ok(())
}

/// WHEN a signature names another channel, THEN the assertion SHALL report
/// nothing.
#[test]
fn test_signatures_arch_8_whole_bus_ignores_another_channel() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-8-other-channels")?;
    support::write_source(&root, CRATE, SOURCE, OTHER_REFERENCES)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_8_whole_bus(&ws);

    // -- Check
    assert!(found.is_empty(), "{found:#?}");
    Ok(())
}

// endregion: --- Drivers
