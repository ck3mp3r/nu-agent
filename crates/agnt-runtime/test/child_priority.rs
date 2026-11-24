use crate::child_priority::{ChildPriority, nice_on_path, wrapped_argv};
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

/// The policy is a value derived from one PATH value, and the spawn it builds
/// carries it: an enabled policy wraps the payload in `nice`, a degraded one
/// spawns the payload bare.
#[test]
fn child_priority_policy_pins_both_probe_outcomes() -> Result<()> {
    // -- Setup & Fixtures
    let root = tempfile::tempdir()?;
    let populated = dir_with_nice(root.path())?;
    let empty = dir_without_nice(root.path())?;

    // -- Exec
    let enabled = ChildPriority::from_path(path_value(&[&empty, &populated])?);
    let degraded = ChildPriority::from_path(path_value(&[&empty])?);

    // -- Check
    assert!(
        enabled.nice_on_path(),
        "a `nice` file on PATH must enable the policy"
    );
    assert!(
        !degraded.nice_on_path(),
        "no `nice` file on PATH must degrade the policy"
    );

    let wrapped = enabled.command("nu");
    assert_eq!(
        wrapped.as_std().get_program().to_string_lossy(),
        "nice",
        "an enabled policy spawns the wrapper"
    );
    assert_eq!(
        args_of(&wrapped),
        vec!["-n".to_string(), "10".to_string(), "nu".to_string()],
        "an enabled policy hands the wrapper the payload"
    );

    let bare = degraded.command("nu");
    assert_eq!(
        bare.as_std().get_program().to_string_lossy(),
        "nu",
        "a degraded policy spawns the payload itself"
    );
    assert!(
        args_of(&bare).is_empty(),
        "a degraded policy adds no arguments"
    );
    Ok(())
}

// region:    --- Test Support

fn args_of(command: &tokio::process::Command) -> Vec<String> {
    command
        .as_std()
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

/// A directory holding a `nice` file: the probe's enabled outcome.
fn dir_with_nice(root: &Path) -> Result<std::path::PathBuf> {
    let populated = root.join("populated");
    fs::create_dir(&populated)?;
    fs::write(populated.join("nice"), "")?;
    Ok(populated)
}

/// A directory holding no `nice` file: the probe's degraded outcome.
fn dir_without_nice(root: &Path) -> Result<std::path::PathBuf> {
    let empty = root.join("empty");
    fs::create_dir(&empty)?;
    Ok(empty)
}

fn path_value(entries: &[&Path]) -> Result<std::ffi::OsString> {
    Ok(std::env::join_paths(entries)?)
}

// endregion: --- Test Support
