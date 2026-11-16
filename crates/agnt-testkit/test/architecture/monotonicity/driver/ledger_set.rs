//! What a ledger file is: the file set the driver reads, and the rows each file
//! records.
//!
//! The per-crate files are the ones the census index declares, plus the
//! assertion table. The index is the census, not a ledger: its tables are typed
//! like a ledger, so reading it would collect crate names as rows.

use std::fs;
use std::path::PathBuf;

use super::{
    BASELINE_DIR, INDEX_FILE, TABLE_FILE, base_ledgers, ledger_paths, read_ledgers, rows_of,
};

use super::super::super::support::{Result, fixture_root};

// region:    --- Fixtures

/// The assertion table a fixture holds: one row.
const TABLE_TEXT: &str =
    "pub const BASELINES: [(&str, &[&str]); 1] = [(ID, &[\"a table row\"])];\n";

/// A per-crate baseline file a fixture holds: one row.
const LEDGER_ROWS_TEXT: &str = "pub const ROWS: &[&str] = &[\"a ledger row\"];\n";

// endregion: --- Fixtures

// region:    --- Drivers

/// WHEN a ledger file records its rows in a `&[&str]` const, THEN every one of
/// its literals SHALL be a row, and a plain string const SHALL NOT be.
#[test]
fn test_monotonicity_rows_of_reads_a_row_slice() -> Result<()> {
    // -- Setup & Fixtures
    let text = "pub const ROWS: &[&str] = &[\n    \"row one\",\n];\n\
                pub const MEMBER_ROWS: &[&str] = &[\"row two\"];\n\
                const FIXTURE_ROOT: &str = \"../../tmp\";\n";

    // -- Exec
    let rows = rows_of(text)?;

    // -- Check
    assert_eq!(rows, vec!["row one", "row two"]);
    Ok(())
}

/// WHEN a ledger file records its rows in the assertion table, THEN every row
/// literal in the table SHALL be a row.
#[test]
fn test_monotonicity_rows_of_reads_the_assertion_table() -> Result<()> {
    // -- Setup & Fixtures
    let text =
        "pub const BASELINES: [(&str, &[&str]); 2] = [(ID, &[]), (OTHER, &[\"row three\"])];\n";

    // -- Exec
    let rows = rows_of(text)?;

    // -- Check
    assert_eq!(rows, vec!["row three"]);
    Ok(())
}

/// WHEN a new crate joins the census and the index gains its name, THEN the
/// ledger set SHALL hold no index file and no crate name: the index is the
/// census table, not a ledger.
///
/// Discriminating run: revert `ledger_paths` to the directory listing and this
/// test fails, reporting four paths with `baselines_arch6/mod.rs` among them.
#[test]
fn test_monotonicity_ledger_set_excludes_the_census_index() -> Result<()> {
    // -- Setup & Fixtures
    let index = "\
        mod agnt_runtime;\n\
        mod agnt_foo;\n\
        pub const ARCH_6_CRATES: [(&str, &[&str]); 2] = [\n\
            (\"agnt-runtime\", agnt_runtime::ROWS),\n\
            (\"agnt-foo\", agnt_foo::ROWS),\n\
        ];\n";
    let root = baseline_fixture(
        "monotonicity_index",
        TABLE_TEXT,
        &[
            (INDEX_FILE, index),
            (
                "agnt_runtime.rs",
                "pub const ROWS: &[&str] = &[\"a runtime row\"];\n",
            ),
            (
                "agnt_foo.rs",
                "pub const ROWS: &[&str] = &[\"a new crate row\"];\n",
            ),
        ],
    )?;

    // -- Exec
    let paths = ledger_paths(&root)?;
    let ledgers = read_ledgers(&root, &paths)?;
    let index_rows = rows_of(index)?;

    // -- Check
    assert!(
        index_rows.contains(&"agnt-foo".to_string()),
        "the index's crate names parse as rows, which is why the index is never read: {index_rows:?}"
    );
    assert_eq!(
        paths.len(),
        3,
        "the table and the two declared crates: {paths:?}"
    );
    assert!(
        !paths
            .iter()
            .any(|path| path.ends_with(&format!("/{INDEX_FILE}"))),
        "the index is not a ledger: {paths:?}"
    );
    let rows: Vec<&String> = ledgers.iter().flat_map(|ledger| ledger.rows()).collect();
    assert_eq!(
        rows.len(),
        3,
        "every declared file's row, and nothing else: {rows:?}"
    );
    for row in ["a table row", "a runtime row", "a new crate row"] {
        assert!(
            rows.iter().any(|held| held.as_str() == row),
            "{row} missing: {rows:?}"
        );
    }
    assert!(
        !rows.iter().any(|held| held.as_str() == "agnt-foo"),
        "a crate name must never be a ledger row: {rows:?}"
    );
    Ok(())
}

/// WHEN a baseline file sits in the index's directory undeclared, THEN the
/// ledger read SHALL fail and name the file: an unread baseline would pass
/// unmonitored.
#[test]
fn test_monotonicity_ledger_paths_rejects_an_undeclared_baseline() -> Result<()> {
    // -- Setup & Fixtures
    let root = baseline_fixture(
        "monotonicity_undeclared",
        TABLE_TEXT,
        &[
            (INDEX_FILE, "mod agnt_runtime;\n"),
            ("agnt_runtime.rs", LEDGER_ROWS_TEXT),
            ("agnt_forgot.rs", LEDGER_ROWS_TEXT),
        ],
    )?;

    // -- Exec
    let error = ledger_paths(&root)
        .err()
        .ok_or("an undeclared baseline file must fail the ledger read")?;

    // -- Check
    assert!(error.to_string().contains("agnt_forgot.rs"), "{error}");
    Ok(())
}

/// WHEN a baseline blob at the branch point does not parse, THEN the base read
/// SHALL fail and name the file, and a blob the revision does not hold SHALL
/// stay exempt.
#[test]
fn test_monotonicity_base_ledgers_fails_on_an_unparsable_blob() -> Result<()> {
    // -- Setup & Fixtures
    let path = "crates/agnt-testkit/test/architecture/baselines_arch6/agnt_runtime.rs";
    let paths = [path.to_string()];

    // -- Exec
    let unparsable = base_ledgers(&paths, "the-base", |_spec| Some("not Rust {{{".to_string()));
    let absent = base_ledgers(&paths, "the-base", |_spec| None);

    // -- Check
    let error = unparsable
        .err()
        .ok_or("an unparsable base blob must fail the read rather than pass")?;
    let message = error.to_string();
    assert!(message.contains("agnt_runtime.rs"), "{message}");
    assert!(message.contains("the-base"), "{message}");
    assert!(
        absent?.is_empty(),
        "a file the base does not hold is the new-file exemption"
    );
    Ok(())
}

// endregion: --- Drivers

// region:    --- Support

/// A fixture crate root: `table` as the assertion table, and `files` as the
/// baseline directory's files, the index among them.
fn baseline_fixture(name: &str, table: &str, files: &[(&str, &str)]) -> Result<PathBuf> {
    let root = fixture_root(name)?;
    let dir = root.join(BASELINE_DIR);
    fs::create_dir_all(&dir)?;
    let table_path = root.join(TABLE_FILE);
    if let Some(parent) = table_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(table_path, table)?;
    for (file, text) in files {
        fs::write(dir.join(file), text)?;
    }
    Ok(root)
}

// endregion: --- Support
