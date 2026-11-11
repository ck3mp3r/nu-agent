use std::path::PathBuf;

/// Read one markdown fixture from this crate's fixture directory.
pub(crate) fn markdown_fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src/fixtures/markdown")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "failed to read markdown fixture {}: {error}",
            path.display()
        )
    })
}
