//! The workspace loader: every `crates/*/src` Rust source, parsed once, and the manifests.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use syn::File;

// region:    --- Types

/// The workspace's sources and manifests.
///
/// Every source is labelled `crate/src/relative/path` relative to the workspace
/// root, which is what a violation site renders.
pub struct Workspace {
    root: PathBuf,
    sources: Vec<(String, String)>,
    parsed: RefCell<HashMap<String, File>>,
}

/// A crate manifest's dependency tables, each key with the line it sits on.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Manifest {
    dependencies: Vec<(String, usize)>,
    dev_dependencies: Vec<(String, usize)>,
    build_dependencies: Vec<(String, usize)>,
}

/// One dependency table of a manifest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Table {
    Dependencies,
    DevDependencies,
    BuildDependencies,
    Other,
}

// endregion: --- Types

// region:    --- Workspace: Constructors

impl Workspace {
    /// Load the workspace this crate belongs to.
    pub fn load() -> Result<Self, io::Error> {
        Self::from_root(workspace_root())
    }

    /// Load the workspace rooted at `root`.
    ///
    /// A fixture tree is loaded the same way a real one is: the loader reads
    /// `crates/*/src` and nothing else, so a rogue fixture cannot reach the
    /// sources of the workspace the suite runs in.
    pub fn from_root(root: impl Into<PathBuf>) -> Result<Self, io::Error> {
        let root = root.into();
        let crates = root.join(CRATES_DIR);
        let mut sources = Vec::new();
        for entry in fs::read_dir(&crates)? {
            let src = entry?.path().join(SOURCES_DIR);
            if src.is_dir() {
                collect_sources(&crates, &src, &mut sources)?;
            }
        }
        sources.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(Self {
            root,
            sources,
            parsed: RefCell::new(HashMap::new()),
        })
    }
}

// endregion: --- Workspace: Constructors

// region:    --- Workspace: Accessors

impl Workspace {
    /// Every loaded source, as (label, text), ordered by label.
    pub fn sources(&self) -> &[(String, String)] {
        &self.sources
    }

    /// The parsed file at `path`, or `None` when the workspace holds no such
    /// source.
    ///
    /// The parse happens once: a checker that reads several items out of one
    /// file asks for the file once per item and pays for one parse.
    pub fn rust_file(&self, path: &str) -> Option<File> {
        if let Some(file) = self.parsed.borrow().get(path) {
            return Some(file.clone());
        }
        let (_, source) = self.sources.iter().find(|(label, _)| label == path)?;
        let file = syn::parse_file(source).ok()?;
        self.parsed
            .borrow_mut()
            .insert(path.to_string(), file.clone());
        Some(file)
    }

    /// The manifest of `krate`, empty when the crate has none.
    pub fn manifest(&self, krate: &str) -> Manifest {
        let path = self.root.join(CRATES_DIR).join(krate).join(MANIFEST_FILE);
        match fs::read_to_string(path) {
            Ok(text) => Manifest::parse(&text),
            Err(_) => Manifest::default(),
        }
    }
}

// endregion: --- Workspace: Accessors

// region:    --- Manifest: Constructors

impl Manifest {
    /// Read a manifest as text: a `Cargo.toml` is not Rust source.
    fn parse(text: &str) -> Self {
        let mut manifest = Self::default();
        let mut table = Table::Other;
        for (index, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(header) = line
                .strip_prefix('[')
                .and_then(|rest| rest.strip_suffix(']'))
            {
                let (owner, subtable) = Table::from_header(header);
                match subtable {
                    Some(key) => {
                        manifest.append(owner, key, index + 1);
                        table = Table::Other;
                    }
                    None => table = owner,
                }
                continue;
            }
            let key = key_of(line);
            if !key.is_empty() {
                manifest.append(table, key, index + 1);
            }
        }
        manifest
    }
}

// endregion: --- Manifest: Constructors

// region:    --- Manifest: Accessors

impl Manifest {
    /// The `[dependencies]` keys, each with its line.
    pub fn dependencies(&self) -> &[(String, usize)] {
        &self.dependencies
    }

    /// The `[dev-dependencies]` keys, each with its line.
    pub fn dev_dependencies(&self) -> &[(String, usize)] {
        &self.dev_dependencies
    }

    /// The `[build-dependencies]` keys, each with its line.
    pub fn build_dependencies(&self) -> &[(String, usize)] {
        &self.build_dependencies
    }
}

// endregion: --- Manifest: Accessors

// region:    --- Manifest: Support

impl Manifest {
    /// Record `key` in `table`, or nowhere when the table is not a dependency
    /// table.
    fn append(&mut self, table: Table, key: &str, line: usize) {
        let entry = (key.to_string(), line);
        match table {
            Table::Dependencies => self.dependencies.push(entry),
            Table::DevDependencies => self.dev_dependencies.push(entry),
            Table::BuildDependencies => self.build_dependencies.push(entry),
            Table::Other => {}
        }
    }
}

impl Table {
    /// The table a header names, and the subtable key it introduces.
    ///
    /// `[dependencies]` names a table outright. `[dependencies.serde]` and
    /// `[target.'cfg(unix)'.dependencies.serde]` name one dependency through a
    /// subtable, so the key is reported at the header's line and the lines below
    /// belong to that dependency, not to the table.
    fn from_header(header: &str) -> (Self, Option<&str>) {
        let segments: Vec<&str> = header.split('.').collect();
        let Some(index) = segments
            .iter()
            .position(|segment| Self::named(segment).holds_dependencies())
        else {
            return (Self::Other, None);
        };
        let owner = Self::named(segments[index]);
        match segments.len() - index {
            1 => (owner, None),
            2 => (owner, Some(segments[index + 1])),
            _ => (Self::Other, None),
        }
    }

    /// The table `segment` names.
    fn named(segment: &str) -> Self {
        match segment {
            DEPENDENCIES => Self::Dependencies,
            DEV_DEPENDENCIES => Self::DevDependencies,
            BUILD_DEPENDENCIES => Self::BuildDependencies,
            _ => Self::Other,
        }
    }

    /// Whether this table holds dependencies.
    fn holds_dependencies(self) -> bool {
        self != Self::Other
    }
}

// endregion: --- Manifest: Support

// region:    --- Support

/// The workspace root, relative to this crate's manifest.
const WORKSPACE_ROOT: &str = "../..";

/// The directory holding the workspace's crates, relative to the root.
const CRATES_DIR: &str = "crates";

/// The directory holding one crate's sources.
const SOURCES_DIR: &str = "src";

/// A crate's manifest file name.
const MANIFEST_FILE: &str = "Cargo.toml";

/// The Rust source extension.
const RUST_SOURCE: &str = "rs";

const DEPENDENCIES: &str = "dependencies";
const DEV_DEPENDENCIES: &str = "dev-dependencies";
const BUILD_DEPENDENCIES: &str = "build-dependencies";

/// The workspace root of the crate this code is compiled into.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(WORKSPACE_ROOT)
}

/// Every Rust source under `dir`, labelled relative to `base`.
fn collect_sources(base: &Path, dir: &Path, out: &mut Vec<(String, String)>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_sources(base, &path, out)?;
        } else if path
            .extension()
            .is_some_and(|extension| extension == RUST_SOURCE)
        {
            let label = path.strip_prefix(base).map_err(io::Error::other)?;
            let label = label.to_string_lossy().replace('\\', "/");
            out.push((label, fs::read_to_string(&path)?));
        }
    }
    Ok(())
}

/// The key a manifest line declares, or an empty string when the line declares
/// none.
///
/// A key may carry an inline table (`serde = { workspace = true }`) or a dotted
/// form (`serde.workspace = true`); both declare `serde`.
fn key_of(line: &str) -> &str {
    let Some((left, _)) = line.split_once('=') else {
        return "";
    };
    let left = left.trim();
    let key = left.split_once('.').map_or(left, |(key, _)| key);
    key.trim()
}

// endregion: --- Support
