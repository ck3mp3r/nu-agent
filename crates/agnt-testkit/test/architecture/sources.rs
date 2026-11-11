//! The loader: source labels, the parse cache, and the manifest tables.

use syn::spanned::Spanned;

use crate::architecture::Workspace;

use super::support::{self, Result};

// region:    --- Fixtures

/// A source every workspace carries.
const KERNEL_SOURCE: &str = "agnt-kernel/src/lib.rs";

/// A manifest naming a dependency in every form the reader must handle.
const FULL_MANIFEST: &str = concat!(
    "[package]\n",
    "name = \"agnt-kernel\"\n",
    "\n",
    "[dependencies]\n",
    "serde = { workspace = true }\n",
    "tokio.workspace = true\n",
    "\n",
    "[dependencies.agnt-runtime]\n",
    "path = \"../agnt-runtime\"\n",
    "\n",
    "[target.'cfg(unix)'.build-dependencies]\n",
    "cc = \"1\"\n",
    "\n",
    "[dev-dependencies]\n",
    "agnt-testkit = { workspace = true }\n",
    "\n",
    "[features]\n",
    "std = []\n",
);

// endregion: --- Fixtures

// region:    --- Drivers

/// WHEN the workspace is loaded, THE sources SHALL be labelled
/// `crate/src/relative/path` and SHALL parse on request; WHEN a file is parsed a
/// second time, THE parse SHALL carry the same item lines — the reported line is
/// what a violation site renders, so a span without a location is a blind check.
#[test]
fn test_sources_workspace_load_labels_and_parses_its_sources() -> Result<()> {
    // -- Setup & Fixtures
    let ws = Workspace::load()?;

    // -- Exec
    let labels: Vec<&str> = ws
        .sources()
        .iter()
        .map(|(label, _)| label.as_str())
        .collect();
    let first = ws.rust_file(KERNEL_SOURCE);
    let again = ws.rust_file(KERNEL_SOURCE);
    let missing = ws.rust_file("agnt-kernel/src/none.rs");
    let source = ws
        .sources()
        .iter()
        .find(|(label, _)| label == KERNEL_SOURCE)
        .map(|(_, text)| text.as_str())
        .ok_or("the kernel source must be loaded")?;

    // -- Check
    assert!(!labels.is_empty(), "the loader found no source at all");
    assert!(
        labels.iter().all(|label| label.contains("/src/")),
        "a label is not crate-relative: {labels:#?}"
    );
    assert!(labels.contains(&KERNEL_SOURCE), "{labels:#?}");
    assert!(
        first.as_ref().is_some_and(|file| !file.items.is_empty()),
        "{KERNEL_SOURCE} did not parse into items"
    );
    assert!(
        missing.is_none(),
        "a source that does not exist must not parse"
    );

    let file = again.ok_or("a parsed file must be returned again")?;
    let item = file.items.first().ok_or("the kernel source holds items")?;
    let line = item.span().start().line;
    assert!(line > 1, "a span carries no location: line {line}");
    let text = source
        .lines()
        .nth(line - 1)
        .ok_or("the reported line must exist in the source")?
        .trim();
    assert!(
        !text.is_empty() && !text.starts_with("//"),
        "the reported line must carry the item: {line}: {text}"
    );
    Ok(())
}

/// WHEN a manifest names dependencies of every form, THE reader SHALL report
/// each under its own table, with the line it sits on.
#[test]
fn test_sources_workspace_manifest_reads_every_dependency_table() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-12-manifest-tables")?;
    support::write_manifest(&root, "agnt-kernel", FULL_MANIFEST)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let manifest = ws.manifest("agnt-kernel");

    // -- Check
    let dependencies: Vec<(String, usize)> = manifest.dependencies().to_vec();
    assert_eq!(
        dependencies,
        [
            ("serde".to_string(), 5),
            ("tokio".to_string(), 6),
            ("agnt-runtime".to_string(), 8),
        ]
    );
    let build_dependencies: Vec<(String, usize)> = manifest.build_dependencies().to_vec();
    assert_eq!(build_dependencies, [("cc".to_string(), 12)]);
    let dev_dependencies: Vec<(String, usize)> = manifest.dev_dependencies().to_vec();
    assert_eq!(dev_dependencies, [("agnt-testkit".to_string(), 15)]);
    Ok(())
}

// endregion: --- Drivers
