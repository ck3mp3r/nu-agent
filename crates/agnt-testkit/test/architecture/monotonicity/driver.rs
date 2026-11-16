//! The git half: the branch point read, and the real ledgers compared to it.

// region:    --- Modules

mod ledger_set;

// endregion: --- Modules

use std::fs;
use std::path::Path;
use std::process::Command;

use syn::{Expr, ExprLit, Item, Lit, Type};

use crate::architecture::{Growth, Ledger, LedgerProbe, ledger_growth};

use super::super::support::Result;

// region:    --- Fixtures

/// The repository path prefix of this crate, trailing separator included.
const REPO_PREFIX: &str = "crates/agnt-testkit/";

/// The directory holding the per-crate ARCH-6 ledger files, relative to this
/// crate.
const BASELINE_DIR: &str = "test/architecture/baselines_arch6";

/// The file holding the assertion-indexed `BASELINES` table, relative to this
/// crate.
const TABLE_FILE: &str = "test/architecture/support.rs";

/// The census index inside [`BASELINE_DIR`]: the table that names the crates,
/// not a ledger. Its tables are typed like a ledger, so it is never read as one.
const INDEX_FILE: &str = "mod.rs";

/// The directory whose sources the census walk reads: a branch that changes it
/// re-measures, and a re-measure may grow.
const WALK_DIR: &str = "crates/agnt-testkit/src/architecture/";

/// The Rust source extension.
const RUST_SOURCE: &str = "rs";

/// The program this driver runs.
const GIT: &str = "git";

/// The ref the branch point is read against.
const MAIN: &str = "main";

// endregion: --- Fixtures

// region:    --- Drivers

/// WHEN the tree records a ledger key the branch point did not, THEN the
/// direction check SHALL fail and name the key.
///
/// The driver cannot fail at HEAD, so the discriminating run takes two edits.
/// At HEAD the walk exemption fires (the branch edits
/// `crates/agnt-testkit/src/architecture/mod.rs`), and the growth this branch
/// carried at `43e32419` — three keys in `agnt_runtime.rs` — was drained by task
/// `4617789b`, so no growth remains to catch. To reproduce the red run:
///
/// 1. Restore the grown ledger, which holds three keys the branch point does
///    not (`is_building`, `mark_building`, `try_built`, all
///    `agnt-runtime/src/conversation/state/provider.rs`):
///    `git show 43e32419:crates/agnt-testkit/test/architecture/baselines_arch6/agnt_runtime.rs > crates/agnt-testkit/test/architecture/baselines_arch6/agnt_runtime.rs`
/// 2. Suppress the walk exemption: make `touches_the_walk` return `false`.
/// 3. `cargo nextest run -p agnt-testkit -E 'test(monotonicity_ledger_growth_absent_on_the_branch)'`
///    — it fails, naming the three keys in `agnt_runtime.rs`.
/// 4. Revert both: `git checkout -- crates/agnt-testkit/test/architecture/baselines_arch6/agnt_runtime.rs`,
///    and undo step 2.
///
/// Step 2 is required because the walk exemption is all-or-nothing: a branch
/// that touches the walk exempts every ledger, not only the crates the change
/// re-measured.
#[test]
fn test_monotonicity_ledger_growth_absent_on_the_branch() -> Result<()> {
    // -- Setup & Fixtures
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let paths = ledger_paths(root)?;
    let tree = read_ledgers(root, &paths)?;

    // -- Exec
    let probe = probe(root);
    if let Some(reason) = probe.skip_reason() {
        println!("skipping the ledger-direction check: {reason}");
        return Ok(());
    }
    let Some(base_rev) = git(root, &["merge-base", MAIN, "HEAD"]) else {
        println!("skipping the ledger-direction check: `main` and HEAD share no branch point");
        return Ok(());
    };
    let base = base_ledgers(&paths, &base_rev, |spec| git(root, &["show", spec]))?;
    let walk_changed = touches_the_walk(root, &base_rev);
    let grown = ledger_growth(&base, &tree, walk_changed);

    // -- Check
    let recorded: usize = tree.iter().map(|ledger| ledger.rows().len()).sum();
    assert!(
        recorded > 0,
        "the driver read no ledger rows at all: an empty read would pass this driver"
    );
    assert!(
        !base.is_empty(),
        "no ledger file exists at {base_rev}: the comparison has no base"
    );
    if walk_changed {
        println!("the branch changes the walk: the direction check is exempt for this branch");
        return Ok(());
    }
    assert!(grown.is_empty(), "{}", report(&grown, &base_rev));
    Ok(())
}

// endregion: --- Drivers

// region:    --- Support

/// The three facts the skip decision reads, probed from git.
fn probe(root: &Path) -> LedgerProbe {
    LedgerProbe {
        git: git(root, &["rev-parse", "--is-inside-work-tree"]).is_some(),
        shallow: git(root, &["rev-parse", "--is-shallow-repository"]).as_deref() == Some("true"),
        main: git(root, &["rev-parse", "--verify", "--quiet", MAIN]).is_some(),
    }
}

/// Whether the branch touches the walk's own sources.
fn touches_the_walk(root: &Path, base_rev: &str) -> bool {
    git(root, &["diff", "--name-only", base_rev])
        .is_some_and(|changed| changed.lines().any(|path| path.starts_with(WALK_DIR)))
}

/// Every ledger file's repository path: the per-crate ARCH-6 files the index
/// declares, and the assertion table.
///
/// The index is the census, not a ledger. Its `ARCH_6_CRATES` and
/// `ARCH_6_MEMBER_CRATES` tables are typed like a ledger, so reading the index
/// would collect crate names as rows and report a new crate as growth in the
/// index. The set is therefore the index's `mod` declarations, and a file the
/// index does not declare fails this read rather than passing unmonitored.
fn ledger_paths(root: &Path) -> Result<Vec<String>> {
    let dir = root.join(BASELINE_DIR);
    let index = fs::read_to_string(dir.join(INDEX_FILE))?;
    let declared = declared_ledgers(&index)?;
    let mut paths = vec![format!("{REPO_PREFIX}{TABLE_FILE}")];
    for name in &declared {
        paths.push(format!("{REPO_PREFIX}{BASELINE_DIR}/{name}"));
    }
    for entry in fs::read_dir(&dir)? {
        let path = entry?.path();
        if !path
            .extension()
            .is_some_and(|extension| extension == RUST_SOURCE)
        {
            continue;
        }
        let name = path.file_name().ok_or("a baseline file has no name")?;
        let name = name.to_string_lossy();
        if name == INDEX_FILE || declared.iter().any(|declared| declared == name.as_ref()) {
            continue;
        }
        return Err(format!(
            "{BASELINE_DIR}/{name} is not declared by the index: the ledger set is the index's \
             modules, and an undeclared baseline file would pass unmonitored"
        )
        .into());
    }
    paths.sort();
    Ok(paths)
}

/// Every baseline module the index declares, by file name.
fn declared_ledgers(index: &str) -> Result<Vec<String>> {
    let file = syn::parse_file(index)
        .map_err(|error| format!("the baseline index does not parse: {error}"))?;
    Ok(file
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Mod(item_mod) if item_mod.content.is_none() => {
                Some(format!("{}.{RUST_SOURCE}", item_mod.ident))
            }
            _ => None,
        })
        .collect())
}

/// The ledger files as the working tree holds them.
fn read_ledgers(root: &Path, paths: &[String]) -> Result<Vec<Ledger>> {
    let mut ledgers = Vec::new();
    for path in paths {
        let relative = path
            .strip_prefix(REPO_PREFIX)
            .ok_or("a ledger path is outside the crate")?;
        let text = fs::read_to_string(root.join(relative))?;
        ledgers.push(Ledger::new(path.as_str(), rows_of(&text)?));
    }
    Ok(ledgers)
}

/// The ledger files as `base_rev` holds them, read through `read`. A revision
/// that does not hold a file leaves it out of the comparison, which is the
/// new-file exemption. A file the revision does hold must parse: skipping an
/// unparsable blob would turn a failed read into that same exemption, and the
/// file's growth would pass silently.
fn base_ledgers(
    paths: &[String],
    base_rev: &str,
    read: impl Fn(&str) -> Option<String>,
) -> Result<Vec<Ledger>> {
    let mut ledgers = Vec::new();
    for path in paths {
        let spec = format!("{base_rev}:{path}");
        let Some(text) = read(&spec) else {
            continue;
        };
        let rows = rows_of(&text)
            .map_err(|error| format!("{path} at {base_rev} does not parse: {error}"))?;
        ledgers.push(Ledger::new(path.as_str(), rows));
    }
    Ok(ledgers)
}

/// Every row `text` records: the string literals of each row-shaped const.
fn rows_of(text: &str) -> Result<Vec<String>> {
    let file = syn::parse_file(text).map_err(|error| format!("not Rust source: {error}"))?;
    let mut rows = Vec::new();
    collect_rows(&file.items, &mut rows);
    Ok(rows)
}

/// Every row in `items`, walking inline modules.
fn collect_rows(items: &[Item], out: &mut Vec<String>) {
    for item in items {
        match item {
            Item::Mod(item_mod) => {
                if let Some((_, nested)) = &item_mod.content {
                    collect_rows(nested, out);
                }
            }
            Item::Const(item_const) if is_ledger_type(&item_const.ty) => {
                literals(&item_const.expr, out);
            }
            _ => {}
        }
    }
}

/// Every string literal in `expr`, following the shapes a row list is written
/// in.
fn literals(expr: &Expr, out: &mut Vec<String>) {
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Str(text),
            ..
        }) => out.push(text.value()),
        Expr::Reference(reference) => literals(&reference.expr, out),
        Expr::Array(array) => array.elems.iter().for_each(|elem| literals(elem, out)),
        Expr::Tuple(tuple) => tuple.elems.iter().for_each(|elem| literals(elem, out)),
        Expr::Paren(paren) => literals(&paren.expr, out),
        _ => {}
    }
}

/// Whether a const's type is a row list: `&[&str]` for one ledger half, or the
/// `[(&str, &[&str]); N]` table for the assertion-indexed rows.
fn is_ledger_type(ty: &Type) -> bool {
    match ty {
        Type::Array(array) => is_table_row(&array.elem),
        Type::Reference(_) => is_row_slice(ty),
        _ => false,
    }
}

/// Whether a type is `&[&str]`.
fn is_row_slice(ty: &Type) -> bool {
    let Type::Reference(reference) = ty else {
        return false;
    };
    is_str_slice(&reference.elem)
}

/// Whether a type is `[&str]`, the slice a row list holds.
fn is_str_slice(ty: &Type) -> bool {
    let Type::Slice(slice) = ty else {
        return false;
    };
    is_str_reference(&slice.elem)
}

/// Whether a type is `(&str, &[&str])`.
fn is_table_row(ty: &Type) -> bool {
    let Type::Tuple(tuple) = ty else {
        return false;
    };
    tuple.elems.len() == 2 && is_str_reference(&tuple.elems[0]) && is_row_slice(&tuple.elems[1])
}

/// Whether a type is `&str`.
fn is_str_reference(ty: &Type) -> bool {
    let Type::Reference(reference) = ty else {
        return false;
    };
    let Type::Path(type_path) = &*reference.elem else {
        return false;
    };
    type_path.path.is_ident("str")
}

/// The trimmed stdout of one git command, or `None` when git is absent or the
/// command fails.
fn git(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new(GIT)
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// The failure message for `grown`.
fn report(grown: &[Growth], base_rev: &str) -> String {
    let listed: Vec<String> = grown.iter().map(ToString::to_string).collect();
    let exemptions = [
        format!(
            "- the ledger file is absent at {base_rev}: a new crate's first census, or a renamed file"
        ),
        format!("- the branch changes the walk under {WALK_DIR}: a re-measure"),
        "- the same source file gave up a row in this comparison: a rename in place".to_string(),
        "- a row of the same detail left another source file in this comparison: a cross-file \
         move"
            .to_string(),
    ];
    format!(
        "the ledger grew against {base_rev}: a baseline only ever shrinks, and a row the branch \
         point did not record is surface or a defect that arrived unrecorded.\n{}\n\
         A growth passes only when it is exempt:\n{}\n\
         Drain the row, or state why the row is not growth.",
        listed.join("\n"),
        exemptions.join("\n"),
    )
}

// endregion: --- Support
