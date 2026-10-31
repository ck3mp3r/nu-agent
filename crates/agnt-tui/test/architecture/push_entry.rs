//! Architecture ban: `push_entry` is the transcript store's sole push API.
//!
//! Design note `6ad10855` §3.3/§4.4: one entry per fact, pushed through one
//! API. The two-block preview work added a second push path (`insert_block_at`)
//! and its caller (`push_block`); both are deleted. This ban keeps them out.

use std::path::{Path, PathBuf};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// The store module that owns the push API.
const STORE: &str = "state/transcript_store.rs";

/// Push APIs the store no longer has. Only the store module may name them.
const FORBIDDEN_PUSH_APIS: [&str; 2] = ["push_block", "insert_block_at"];

#[test]
fn test_architecture_push_entry_is_the_sole_push_api() -> Result<()> {
    // -- Setup & Fixtures
    let sources = module_sources()?;

    // -- Exec
    let violations: Vec<String> = sources
        .iter()
        .flat_map(|(module, source)| push_api_violations(module, source))
        .collect();

    // -- Check
    assert!(
        violations.is_empty(),
        "the store's push surface must be exactly push_entry: {violations:#?}"
    );
    Ok(())
}

#[test]
fn test_architecture_push_entry_ban_flags_a_rogue_push_path() -> Result<()> {
    // -- Setup & Fixtures: a module that pushes through the deleted APIs.
    let rogue = "state/rogue.rs";
    let source = concat!(
        "fn rogue(store: &mut TranscriptStore, block: Block) {\n",
        "    store.push_block(block);\n",
        "    store.insert_block_at(1, block);\n",
        "}\n",
    );

    // -- Exec & Check: the rogue module is flagged once per forbidden API.
    assert_eq!(push_api_violations(rogue, source).len(), 2);

    // -- Exec & Check: the same source is legal inside the store module.
    assert!(push_api_violations(STORE, source).is_empty());
    Ok(())
}

// region:    --- Test Support

/// Every production module of the crate, as (path relative to `src`, source).
fn module_sources() -> Result<Vec<(String, String)>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rust_files(&root, &mut files)?;
    files
        .into_iter()
        .map(|file| {
            let module = file
                .strip_prefix(&root)?
                .to_string_lossy()
                .replace('\\', "/");
            let source = std::fs::read_to_string(&file)?;
            Ok((module, source))
        })
        .collect()
}

fn collect_rust_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_rust_files(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
    Ok(())
}

fn push_api_violations(module: &str, source: &str) -> Vec<String> {
    if module == STORE {
        return Vec::new();
    }
    source
        .lines()
        .enumerate()
        .filter(|(_, line)| FORBIDDEN_PUSH_APIS.iter().any(|api| line.contains(api)))
        .map(|(index, line)| format!("{module}:{}: {}", index + 1, line.trim()))
        .collect()
}

// endregion: --- Test Support
