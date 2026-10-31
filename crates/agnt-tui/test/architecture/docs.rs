//! Architecture ban: no docs page claims a compaction layer without code.
//!
//! The help page and the docs pages describe what rig-memory does. Each string
//! in [`FICTION_STRINGS`] named a compaction layer the code never had; the
//! layer is deleted, and no page may bring the name back. The positive
//! contract — the wired knobs, their rig defaults, and the deferred note —
//! lives in `agnt-plugin`'s docs-contract test.

use std::path::{Path, PathBuf};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Fixtures

/// The pages that describe compaction, relative to the workspace root.
const DOCS_PAGES: [&str; 4] = [
    "README.md",
    "crates/agnt-tui/src/runtime/help/help.md",
    "docs/configuration.md",
    "docs/usage.md",
];

/// Strings no docs page may carry: each names a compaction layer with no code
/// behind it.
const FICTION_STRINGS: [&str; 5] = [
    "sliding_window",
    "token_truncate",
    "fallback_strategies",
    "hysteresis",
    "compaction strateg",
];

// endregion: --- Fixtures

// region:    --- Bans

/// WHEN the compaction docs pages are scanned, THE pages SHALL carry no
/// compaction-strategy name: the layer has no code.
#[test]
fn test_architecture_docs_claim_no_compaction_strategy_layer() -> Result<()> {
    // -- Setup & Fixtures
    let pages = docs_pages()?;

    // -- Exec
    let violations: Vec<String> = pages
        .iter()
        .flat_map(|(page, text)| fiction_violations(page, text))
        .collect();

    // -- Check
    assert_eq!(pages.len(), DOCS_PAGES.len(), "every page must be read");
    assert!(
        violations.is_empty(),
        "a docs page claims a compaction layer with no code: {violations:#?}"
    );
    Ok(())
}

/// WHEN the ban runs on a rogue page, THE check SHALL flag every fiction name,
/// and THE landed wording SHALL pass.
#[test]
fn test_architecture_docs_fiction_ban_flags_a_rogue_page() -> Result<()> {
    // -- Setup & Fixtures
    let rogue = "docs/rogue.md";

    // -- Exec & Check: every fiction name is flagged.
    for needle in FICTION_STRINGS {
        let page = format!("Auto compaction follows the {needle} policy.\n");
        assert_eq!(fiction_violations(rogue, &page).len(), 1, "{needle}");
    }

    // -- Exec & Check: the landed wording passes.
    let landed = "Every key is optional: a key left out keeps the rig default.\n";
    assert!(fiction_violations(rogue, landed).is_empty());
    Ok(())
}

// endregion: --- Bans

// region:    --- Test Support

/// The workspace root: this crate sits two levels below it.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every page the ban scans, as (workspace-relative path, text).
fn docs_pages() -> Result<Vec<(String, String)>> {
    DOCS_PAGES
        .iter()
        .map(|page| {
            let text = std::fs::read_to_string(workspace_root().join(page))?;
            Ok((page.to_string(), text))
        })
        .collect()
}

/// Every fiction string one page carries, as `page: needle`.
fn fiction_violations(page: &str, text: &str) -> Vec<String> {
    let text = text.to_ascii_lowercase();
    FICTION_STRINGS
        .iter()
        .filter(|needle| text.contains(*needle))
        .map(|needle| format!("{page}: {needle}"))
        .collect()
}

// endregion: --- Test Support
