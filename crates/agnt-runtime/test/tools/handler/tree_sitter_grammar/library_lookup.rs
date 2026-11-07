//! `find_compiled_library` lookup tests (no grammar install needed).

use super::*;

/// The grammar lookup searches the injected cache dir, keeping the
/// `tree-sitter/lib` layout the tree-sitter CLI uses.
#[test]
fn find_compiled_library_searches_the_injected_cache_dir() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let grammar_dir = temp.path().join("tree-sitter-rust");
    std::fs::create_dir_all(&grammar_dir)?;
    let cache_dir = temp.path().join("cache").join("tree-sitter");
    let lib_dir = cache_dir.join("lib");
    std::fs::create_dir_all(&lib_dir)?;
    let lib = lib_dir.join(format!("rust.{}", library_extension()));
    std::fs::write(&lib, b"placeholder")?;

    // -- Exec
    let found = find_compiled_library(&grammar_dir, "rust", Some(&cache_dir));

    // -- Check
    assert_eq!(found.as_deref(), Some(lib.as_path()));
    Ok(())
}

/// An injected cache dir is searched alone: the lookup never falls back to the
/// process environment's cache.
#[test]
fn find_compiled_library_does_not_fall_back_when_a_cache_dir_is_injected() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let grammar_dir = temp.path().join("tree-sitter-nothing");
    std::fs::create_dir_all(&grammar_dir)?;
    let empty_cache = temp.path().join("empty-cache");
    std::fs::create_dir_all(&empty_cache)?;

    // -- Exec
    let found = find_compiled_library(&grammar_dir, "nothing", Some(&empty_cache));

    // -- Check
    assert!(
        found.is_none(),
        "an injected cache dir must not fall back: {found:?}"
    );
    Ok(())
}
