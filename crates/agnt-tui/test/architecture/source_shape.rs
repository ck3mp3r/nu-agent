//! Architecture bans: the production sources' shape.
//!
//! Four bans pin what a production file may not carry: no `#[cfg(test)]`
//! function, no killed string-sniffing shape, no status derived from a result's
//! text, and the call-key derivation nowhere but `ToolCallId::of`. Each is a
//! workspace-wide source grep — a backdoor, a sniff, or a second derivation is a
//! defect wherever it lands.
//!
//! Test-only hooks: a `#[cfg(test)]` `fn` in a production file is a backdoor
//! into private state — the test reads what the API does not expose. The legal
//! form is the module declaration, `#[cfg(test)]` on a `mod` that points into
//! `test/`. When a test needs more than the API exposes, the API is missing a
//! method. There is no allowlist: the pre-existing inline-test modules in
//! `agnt-core`'s handler tree (`authz_gate`, `dispatch`) already declare
//! `mod ..._test;`, so they pass unchanged.
//!
//! Sniffing: the killed shapes are the tool-name prefix test
//! (`starts_with("edit`), the glyph-string lane match (`"╎"`/`"⚙"` compared as
//! text), and the status-line extract (`tool[`). A call's family is classified
//! once at construction through `ToolKind::from_name`, the glyphs live in the
//! render crate's catalog and never travel back as data, and hydration replays
//! the session log instead of re-parsing persisted rows.
//!
//! Status: a tool call's status is the `ToolStatus` variant the store carries,
//! never a reading of its result text. The persisted text classifications — the
//! not-run census, the permission-denial prefix, the closed-call marker, the
//! error-kind status table, and the snapshot's not-run flag — are gone, and no
//! production file may bring one back. The placeholder DETECTION the LLM-facing
//! write needs stays mechanical; it never feeds a status.
//!
//! Call-key derivation: a key built from a tool name and its arguments is
//! derived in exactly one place, `ToolCallId::of`
//! (`agnt-core/src/protocol/events.rs`). Every other site routes through that
//! constructor.
//!
//! Scoped out, and deliberately staying:
//! - `PermissionRequestContext.tool_key`: the `String` wire key. Its sites route
//!   through `ToolCallId::of`, so it is not a re-derivation.
//! - `context.tool`: the display-formatted tool name the prompt carries.
//! - `UiMessageSnapshot`'s role strings (`"user"`, `"tool"`, `"tool_display"`):
//!   the transport's role tags, matched once when a snapshot is projected.
//! - the handler error kinds (`"cancelled"`, `"denied"`): the LLM-facing and
//!   error-display vocabulary. The status is a variant; this text is content.

use std::path::Path;

use super::sources_under;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Fixtures

/// The attribute a production file may carry only on a `mod` declaration.
const CFG_TEST: &str = "#[cfg(test)]";

/// The workspace's crates directory, relative to this crate's manifest.
const CRATES_DIR: &str = "../..";

/// The one module allowed to derive a call key from a tool name.
const CALL_KEY_CONSTRUCTOR: &str = "agnt-core/src/protocol/events.rs";

/// The call-key derivation expression, split so this file does not match its
/// own ban.
const CALL_KEY_DERIVATION: [&str; 3] = ["format!(", "\"{tool_name}", "\\n"];

/// The killed string-sniffing shapes: a tool-name prefix test, a rendered lane
/// glyph compared as text, and a persisted status-line extract.
const KILLED_SNIFFING: [&str; 8] = [
    "starts_with(\"edit",
    "starts_with('╎",
    "starts_with(\"╎",
    "starts_with('⚙",
    "starts_with(\"⚙",
    "contains('⚙",
    "contains(\"⚙",
    "tool[",
];

/// The killed status-from-text shapes: the not-run census, the permission-denial
/// prefix, the closed-call marker, the error-kind status table, and the
/// snapshot's not-run flag. A status is a `ToolStatus` variant; none of these
/// may come back.
const KILLED_STATUS_SNIFFING: [&str; 6] = [
    "is_did_not_run_result",
    "starts_with(\"Permission denied\"",
    "INTERRUPTED_RESULT",
    "NOT_RUN_ERROR_KINDS",
    "is_not_run_kind",
    "tool_not_run",
];

/// The module that reads the status variants: hydration matches the enum, so
/// the typed read exists where the rows are built.
const STATUS_READER: &str = "agnt-core/src/session/resolver.rs";

/// The status variant the resolver must match on: the cancelled class's arm.
const STATUS_VARIANT: &str = "ToolStatus::Cancelled";

// endregion: --- Fixtures

// region:    --- Bans

/// WHEN the workspace's production sources are scanned, THE code SHALL carry no
/// `#[cfg(test)]` function, SHALL contain no killed sniffing shape, and SHALL
/// derive a call key only in `ToolCallId::of`.
#[test]
fn test_architecture_production_sources_carry_no_test_only_hooks() -> Result<()> {
    // -- Setup & Fixtures
    let sources = workspace_sources()?;

    // -- Exec
    let accessors: Vec<String> = sources
        .iter()
        .flat_map(|(module, source)| accessor_violations(module, source))
        .collect();
    let sniffs: Vec<String> = sources
        .iter()
        .flat_map(|(module, source)| sniffing_violations(module, source))
        .collect();
    let status_sniffs: Vec<String> = sources
        .iter()
        .flat_map(|(module, source)| status_sniffing_violations(module, source))
        .collect();
    let derivations = derivation_sites(&sources);

    // -- Check
    assert!(
        accessors.is_empty(),
        "a production file carries a `#[cfg(test)]` fn: {accessors:#?}"
    );
    assert!(
        sniffs.is_empty(),
        "a killed sniffing shape survives: {sniffs:#?}"
    );
    assert!(
        status_sniffs.is_empty(),
        "a killed status-from-text shape survives: {status_sniffs:#?}"
    );
    let status_reads: Vec<&(String, String)> = sources
        .iter()
        .filter(|(module, _)| module == STATUS_READER)
        .collect();
    assert_eq!(
        status_reads.len(),
        1,
        "the status reader must be scanned: {STATUS_READER}"
    );
    assert!(
        status_reads[0].1.contains(STATUS_VARIANT),
        "the resolver must match the status variant: {STATUS_VARIANT}"
    );
    assert_eq!(
        derivations.len(),
        1,
        "the call-key derivation must appear exactly once: {derivations:#?}"
    );
    assert!(
        derivations[0].starts_with(CALL_KEY_CONSTRUCTOR),
        "the call-key derivation belongs to `ToolCallId::of`: {derivations:#?}"
    );
    Ok(())
}

/// WHEN a rogue fixture carries a test-only function, a killed sniffing shape,
/// or a second call-key derivation, THE checks SHALL flag it, and THE legal
/// forms SHALL pass.
#[test]
fn test_architecture_source_shape_bans_flag_rogue_fixtures() -> Result<()> {
    // -- Setup & Fixtures: a production accessor behind `#[cfg(test)]` and the
    // legal module declaration it must not be confused with.
    let rogue = "widgets/rogue.rs";
    let accessor = concat!(
        "#[cfg(test)]\n",
        "fn pane_width(&self) -> usize {\n",
        "    self.width\n",
        "}\n",
    );
    let declaration = concat!(
        "#[cfg(test)]\n",
        "#[path = \"../test/foo.rs\"]\n",
        "mod foo_test;\n",
    );

    // -- Exec & Check: the accessor is flagged, the declaration is legal.
    assert_eq!(accessor_violations(rogue, accessor).len(), 1);
    assert!(accessor_violations(rogue, declaration).is_empty());

    // -- Exec & Check: every killed sniffing shape is flagged.
    for needle in KILLED_SNIFFING {
        let source = format!("fn rogue(text: &str) -> bool {{\n    text.{needle}\n}}\n");
        assert_eq!(sniffing_violations(rogue, &source).len(), 1, "{needle}");
    }
    assert!(sniffing_violations(rogue, "fn rogue() -> bool {\n    true\n}\n").is_empty());

    // -- Exec & Check: every killed status-from-text shape is flagged.
    for needle in KILLED_STATUS_SNIFFING {
        let source = format!("fn rogue(text: &str) -> bool {{\n    {needle}(text)\n}}\n");
        assert_eq!(
            status_sniffing_violations(rogue, &source).len(),
            1,
            "{needle}"
        );
    }
    assert!(status_sniffing_violations(rogue, "fn rogue() -> bool {\n    true\n}\n").is_empty());

    // -- Exec & Check: a second derivation joins the constructor as a second
    // site, so the workspace-wide pin cannot miss it.
    let mut sources = workspace_sources()?;
    sources.push((
        rogue.to_string(),
        format!(
            "fn rogue(name: &str) -> String {{\n    {}\n}}\n",
            call_key_needle()
        ),
    ));
    let derivations = derivation_sites(&sources);
    assert_eq!(derivations.len(), 2, "{derivations:#?}");
    assert!(derivations.iter().any(|site| site.starts_with(rogue)));
    Ok(())
}

// endregion: --- Bans

// region:    --- Test Support

/// Every production module of the workspace, as (path relative to `crates`,
/// source).
fn workspace_sources() -> Result<Vec<(String, String)>> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(CRATES_DIR)
        .join("crates");
    let mut sources = Vec::new();
    for entry in std::fs::read_dir(&crates)? {
        let src = entry?.path().join("src");
        if src.is_dir() {
            sources.extend(sources_under(&src, &crates)?);
        }
    }
    Ok(sources)
}

/// The item a `#[cfg(test)]` attribute lands on: the first line after it that
/// is not blank, not a comment, and not another attribute.
fn item_after_attribute<'a>(lines: &[&'a str], from: usize) -> Option<(usize, &'a str)> {
    let mut index = from + 1;
    while let Some(line) = lines.get(index) {
        let text = line.trim();
        if text.is_empty() || text.starts_with("//") || text.starts_with("#[") {
            index += 1;
            continue;
        }
        return Some((index, text));
    }
    None
}

/// Whether an item line declares a function, after any leading modifiers.
fn is_fn_item(line: &str) -> bool {
    const MODIFIERS: [&str; 7] = [
        "pub",
        "pub(crate)",
        "pub(super)",
        "async",
        "const",
        "unsafe",
        "extern",
    ];
    line.split_whitespace()
        .find(|word| !MODIFIERS.contains(word))
        .is_some_and(|word| word == "fn")
}

/// Every `#[cfg(test)]` function in one production module.
fn accessor_violations(module: &str, source: &str) -> Vec<String> {
    let lines: Vec<&str> = source.lines().collect();
    lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.trim() == CFG_TEST)
        .filter_map(|(index, _)| item_after_attribute(&lines, index))
        .filter(|(_, item)| is_fn_item(item))
        .map(|(index, item)| format!("{module}:{}: {item}", index + 1))
        .collect()
}

/// Every killed sniffing shape in one production module.
fn sniffing_violations(module: &str, source: &str) -> Vec<String> {
    source
        .lines()
        .enumerate()
        .filter(|(_, line)| KILLED_SNIFFING.iter().any(|needle| line.contains(needle)))
        .map(|(index, line)| format!("{module}:{}: {}", index + 1, line.trim()))
        .collect()
}

/// Every killed status-from-text shape in one production module.
fn status_sniffing_violations(module: &str, source: &str) -> Vec<String> {
    source
        .lines()
        .enumerate()
        .filter(|(_, line)| {
            KILLED_STATUS_SNIFFING
                .iter()
                .any(|needle| line.contains(needle))
        })
        .map(|(index, line)| format!("{module}:{}: {}", index + 1, line.trim()))
        .collect()
}

/// The expression the call-key derivation ban searches for.
fn call_key_needle() -> String {
    CALL_KEY_DERIVATION.concat()
}

/// Every call-key derivation site, as `module:line`.
fn derivation_sites(sources: &[(String, String)]) -> Vec<String> {
    let needle = call_key_needle();
    sources
        .iter()
        .flat_map(|(module, source)| {
            source
                .lines()
                .enumerate()
                .filter(|(_, line)| line.contains(&needle))
                .map(|(index, _)| format!("{module}:{}", index + 1))
                .collect::<Vec<_>>()
        })
        .collect()
}

// endregion: --- Test Support
