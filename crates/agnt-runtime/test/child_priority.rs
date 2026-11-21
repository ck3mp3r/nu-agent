use crate::child_priority::{nice_on_path, wrapped_argv};
use std::ffi::OsStr;
use std::fs;
use std::path::Path;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn child_priority_wrapped_argv_enabled_prefixes_nice() {
    // -- Setup & Fixtures
    let program = "nu";

    // -- Exec
    let (wrapped, argv) = wrapped_argv(program, true);

    // -- Check
    assert_eq!(wrapped, "nice");
    assert_eq!(argv, vec!["-n", "10", program]);
}

#[test]
fn child_priority_wrapped_argv_disabled_is_bare() {
    // -- Setup & Fixtures
    let program = "nu";

    // -- Exec
    let (wrapped, argv) = wrapped_argv(program, false);

    // -- Check
    assert_eq!(wrapped, program);
    assert!(
        argv.is_empty(),
        "a bare spawn queues no arguments: {argv:?}"
    );
}

/// The probe decides the policy from one PATH value: it is enabled only when an
/// entry of that value holds a `nice` file. An empty entry never resolves to the
/// working directory.
#[test]
fn child_priority_nice_on_path_requires_a_nice_file() -> Result<()> {
    // -- Setup & Fixtures
    let root = tempfile::tempdir()?;
    let empty = root.path().join("empty");
    let populated = root.path().join("populated");
    fs::create_dir(&empty)?;
    fs::create_dir(&populated)?;
    fs::write(populated.join("nice"), "")?;

    // -- Exec & Check
    assert!(
        !nice_on_path(path_value(&[&empty])?),
        "a directory holding no `nice` must degrade the policy"
    );
    assert!(
        nice_on_path(path_value(&[&populated])?),
        "a directory holding `nice` must enable the policy"
    );
    assert!(
        nice_on_path(path_value(&[&empty, &populated])?),
        "every entry of the value is searched"
    );
    assert!(
        !nice_on_path(OsStr::new("")),
        "an empty value must not resolve to the working directory"
    );
    Ok(())
}

// region:    --- Test Support

fn path_value(entries: &[&Path]) -> Result<std::ffi::OsString> {
    Ok(std::env::join_paths(entries)?)
}

// endregion: --- Test Support
