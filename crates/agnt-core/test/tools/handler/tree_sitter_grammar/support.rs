//! Shared fixtures for the grammar-backed AST tool tests.

use std::ffi::OsString;
use std::path::PathBuf;

pub(crate) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Fixtures

/// The shared-library extension the host platform uses for grammars.
pub(crate) fn library_extension() -> &'static str {
    if cfg!(target_os = "windows") {
        "dll"
    } else if cfg!(target_os = "macos") {
        "dylib"
    } else {
        "so"
    }
}

/// The host's compiled `rust` grammar, when the OS cache holds one.
pub(crate) fn host_rust_grammar() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let candidate = PathBuf::from(home)
        .join(".cache")
        .join("tree-sitter")
        .join("lib")
        .join(format!("rust.{}", library_extension()));
    candidate.is_file().then_some(candidate)
}

/// A private tree-sitter environment, restored on drop.
pub(crate) struct GrammarEnv {
    _root: tempfile::TempDir,
    previous_tree_sitter_dir: Option<OsString>,
    previous_xdg_cache_home: Option<OsString>,
}

impl GrammarEnv {
    /// Install the private environment. Returns `None` when the host has no
    /// compiled `rust` grammar to copy.
    pub(crate) fn install() -> Result<Option<Self>> {
        let Some(grammar) = host_rust_grammar() else {
            return Ok(None);
        };
        let root = tempfile::tempdir()?;

        // A parser directory holding a `tree-sitter-rust` grammar repo.
        let parsers = root.path().join("parsers");
        std::fs::create_dir_all(parsers.join("tree-sitter-rust"))?;

        // The tree-sitter config that names that parser directory.
        let config_dir = root.path().join("config");
        std::fs::create_dir_all(&config_dir)?;
        std::fs::write(
            config_dir.join("config.json"),
            serde_json::json!({ "parser-directories": [parsers] }).to_string(),
        )?;

        // The OS grammar cache holding the copied compiled grammar.
        let lib_dir = root.path().join("cache").join("tree-sitter").join("lib");
        std::fs::create_dir_all(&lib_dir)?;
        std::fs::copy(
            &grammar,
            lib_dir.join(format!("rust.{}", library_extension())),
        )?;

        let previous_tree_sitter_dir = std::env::var_os("TREE_SITTER_DIR");
        let previous_xdg_cache_home = std::env::var_os("XDG_CACHE_HOME");
        unsafe {
            std::env::set_var("TREE_SITTER_DIR", &config_dir);
            std::env::set_var("XDG_CACHE_HOME", root.path().join("cache"));
        }

        Ok(Some(Self {
            _root: root,
            previous_tree_sitter_dir,
            previous_xdg_cache_home,
        }))
    }
}

impl Drop for GrammarEnv {
    fn drop(&mut self) {
        match self.previous_tree_sitter_dir.take() {
            Some(value) => unsafe { std::env::set_var("TREE_SITTER_DIR", value) },
            None => unsafe { std::env::remove_var("TREE_SITTER_DIR") },
        }
        match self.previous_xdg_cache_home.take() {
            Some(value) => unsafe { std::env::set_var("XDG_CACHE_HOME", value) },
            None => unsafe { std::env::remove_var("XDG_CACHE_HOME") },
        }
    }
}

/// A small Rust source with two functions and three `Status` occurrences.
const SOURCE: &str = "struct Status { ready: bool }\n\nfn process(status: Status) -> bool {\n    status.ready\n}\n\nfn main() {\n    let s = Status { ready: true };\n    process(s);\n}\n";

/// Write `SOURCE` into a tempdir and return the directory and file path.
pub(crate) fn write_source() -> Result<(tempfile::TempDir, PathBuf)> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("sample.rs");
    std::fs::write(&path, SOURCE)?;
    Ok((dir, path))
}

// endregion: --- Fixtures
