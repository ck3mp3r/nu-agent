//! The file-name marker check: a source file's name describes what it holds,
//! never its implementation status.
//!
//! The naming law bans a name that encodes shape — `docs/design-choices.md` §6
//! records the `*_impl` function form. A file name is the same smell, and it is
//! cheaper to fix: the subject here is a path, not a parsed span, so this is the
//! one check that walks paths instead of Rust source.
//!
//! The measured set is every `.rs` path under `crates/` — including a crate's
//! `test/` tree, `build.rs` and `benches/`, which the census never parses. A name
//! is a name wherever the file sits, so the walk covers the directory rather than
//! the parsed source set.
//!
//! Zero tolerance, deliberately: names are cheap to fix, and a file-name ledger
//! would reintroduce the self-service growth hole the baseline direction exists
//! to close. The marker list is maintained by ruling against a clean tree, and a
//! violation has no recording path.

use std::io;

use crate::architecture::report::Violation;
use crate::architecture::sources::Workspace;

// region:    --- Functions

/// WHEN a Rust path under `crates/` carries one of the banned implementation
/// markers in its file name, THEN the check SHALL report that path.
pub fn marked_source_filenames(ws: &Workspace) -> io::Result<Vec<Violation>> {
    let mut violations = Vec::new();
    for path in ws.rust_paths()? {
        let Some(name) = path.rsplit('/').next() else {
            continue;
        };
        for marker in MARKERS {
            if marker.matches(name) {
                violations.push(Violation {
                    assertion: ASSERTION_FILENAMES,
                    site: path.clone(),
                    detail: format!(
                        "`{name}` carries the `{}` implementation marker",
                        marker.word
                    ),
                });
            }
        }
    }
    Ok(violations)
}

// endregion: --- Functions

// region:    --- Support

/// The assertion this module reports.
const ASSERTION_FILENAMES: &str = "NAMES";

/// The banned file-name markers, maintained like the vocabulary term lists: a
/// marker joins by ruling against a clean tree.
const MARKERS: [Marker; 2] = [
    Marker {
        word: "impl",
        shape: Shape::Trailing,
    },
    Marker {
        word: "impl",
        shape: Shape::Leading,
    },
];

/// One banned marker: the word, and where in the file's stem it must not sit.
struct Marker {
    /// The word a file name must not carry.
    word: &'static str,
    /// Where the word would sit.
    shape: Shape,
}

/// The two places a marker word can sit in a stem.
#[derive(Clone, Copy)]
enum Shape {
    /// The stem ends in `_<word>`, e.g. `loop_impl.rs`.
    Trailing,
    /// The stem begins with `<word>_`, e.g. `impl_enum.rs`.
    Leading,
}

impl Marker {
    /// Whether `name` carries this marker.
    fn matches(&self, name: &str) -> bool {
        let stem = name.strip_suffix(".rs").unwrap_or(name);
        match self.shape {
            Shape::Trailing => stem
                .strip_suffix(self.word)
                .is_some_and(|head| head.ends_with('_')),
            Shape::Leading => stem
                .strip_prefix(self.word)
                .is_some_and(|rest| rest.starts_with('_')),
        }
    }
}

// endregion: --- Support
