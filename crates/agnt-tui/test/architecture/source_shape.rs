//! Architecture bans: the production sources' shape.
//!
//! Four bans pin what a production file may not carry: no `#[cfg(test)]`
//! function, no killed string-sniffing shape, no status derived from a result's
//! text, and the call-key derivation nowhere but `ToolCallId::new`. Each is a
//! workspace-wide source grep — a backdoor, a sniff, or a second derivation is a
//! defect wherever it lands.
//!
//! Test-only hooks: a `#[cfg(test)]` `fn` in a production file is a backdoor
//! into private state — the test reads what the API does not expose. The legal
//! form is the module declaration, `#[cfg(test)]` on a `mod` that points into
//! `test/`. When a test needs more than the API exposes, the API is missing a
//! method. There is no allowlist: the pre-existing inline-test modules in
//! `agnt-runtime`'s handler tree (`authz_gate`, `dispatch`) already declare
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
//! derived in exactly one place, `ToolCallId::new`
//! (`agnt-kernel/src/records.rs`). Every other site routes through that
//! constructor.
//!
//! Scoped out, and deliberately staying:
//! - `PermissionRequestContext.tool_key`: the `String` wire key. Its sites route
//!   through `ToolCallId::new`, so it is not a re-derivation.
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
const CALL_KEY_CONSTRUCTOR: &str = "agnt-kernel/src/records.rs";

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
const STATUS_READER: &str = "agnt-runtime/src/session/resolver/hydration.rs";

/// The typed status read the resolver must perform: hydration takes the
/// variant the store recorded, never a reading of the row's text.
const STATUS_READ: &str = "ToolStatusMap";

// endregion: --- Fixtures

// region:    --- Bans

/// WHEN the workspace's production sources are scanned, THE code SHALL carry no
/// `#[cfg(test)]` function, SHALL contain no killed sniffing shape, and SHALL
/// derive a call key only in `ToolCallId::new`.
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
        status_reads[0].1.contains(STATUS_READ),
        "the resolver must read the typed status map: {STATUS_READ}"
    );
    assert_eq!(
        derivations.len(),
        1,
        "the call-key derivation must appear exactly once: {derivations:#?}"
    );
    assert!(
        derivations[0].starts_with(CALL_KEY_CONSTRUCTOR),
        "the call-key derivation belongs to `ToolCallId::new`: {derivations:#?}"
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

    // -- Exec & Check: a consumer naming a gated command is flagged, for either
    // command of the pair.
    for command in GATED_COMMANDS {
        let sources: Vec<(String, String)> = AVAILABILITY_CONSUMERS
            .iter()
            .map(|module| {
                (
                    module.to_string(),
                    format!(
                        "fn rogue(command: SlashCommand) -> bool {{\n    command == SlashCommand::{command}\n}}\n"
                    ),
                )
            })
            .collect();
        let leaks = gate_leaks(&sources);
        assert_eq!(
            leaks.len(),
            AVAILABILITY_CONSUMERS.len(),
            "a consumer naming `{command}` must be flagged: {leaks:?}"
        );
    }
    assert!(gate_leaks(&[]).is_empty());

    // -- Exec & Check: a second derivation of the undo's arm is a second site, so
    // the pin cannot miss it.
    let second_derivation = "agnt-tui/src/state/availability_again.rs";
    let arm = AVAILABILITY_ARMS
        .iter()
        .find(|(arm, _)| arm.starts_with("Uncompact"))
        .ok_or("the undo must be a gated command")?;
    let source = concat!(
        "fn rogue(command: SlashCommand) -> Availability {\n",
        "    match command {\n",
        "        SlashCommand::Uncompact => Availability::Available,\n",
        "        _ => Availability::Available,\n",
        "    }\n",
        "}\n",
    )
    .to_string();
    let sites = arm_sites(&[(second_derivation.to_string(), source)], arm.0);
    assert_eq!(sites.len(), 1, "{sites:#?}");
    assert!(sites[0].0.starts_with(second_derivation));
    Ok(())
}

/// The files whose behaviour a gated command must not touch: the palette's rows,
/// the palette's drawer, and the submit lane. Each derives what it shows and
/// refuses from `CommandAvailability`, so a third gated command adds one arm to
/// that impl and nothing here.
const AVAILABILITY_CONSUMERS: [&str; 3] = [
    "agnt-tui/src/interaction/reducer/support.rs",
    "agnt-tui/src/interaction/reducer/submit.rs",
    "agnt-tui/src/runtime/panels.rs",
];

/// The one file that maps a command to its availability.
const AVAILABILITY_DERIVATION: &str = "agnt-tui/src/state/status.rs";

/// Every gated command: the arm it carries in the availability impl, and the pair
/// field that arm reads. The arm is the one place a command is mapped to whether
/// it may run, so a second derivation anywhere carries the arm's text.
const AVAILABILITY_ARMS: [(&str, &str); 2] = [
    ("Compact => Availability", "command_availability.compact"),
    (
        "Uncompact => Availability",
        "command_availability.uncompact",
    ),
];

/// The commands a consumer may not name: naming one is deciding for it.
const GATED_COMMANDS: [&str; 2] = ["Compact", "Uncompact"];

/// WHEN a command is gated, THEN THE gate SHALL live in one availability impl and
/// SHALL appear NOWHERE in the consumers: no consumer names a command to decide
/// whether it may run, so a third gated command touches only its own arm and its
/// own field of the pair.
#[test]
fn test_architecture_command_gates_live_in_one_availability_impl() -> Result<()> {
    // -- Setup & Fixtures: every production source of the workspace.
    let sources = workspace_sources()?;

    // -- Exec
    let sites: Vec<(String, String)> = AVAILABILITY_ARMS
        .iter()
        .flat_map(|(arm, _)| arm_sites(&sources, arm))
        .collect();
    let leaks = gate_leaks(&sources);

    // -- Check: each gated command's mapping exists once, in the availability
    // impl, and reads its own field of the pair.
    for (arm, field) in AVAILABILITY_ARMS {
        let sites: Vec<&(String, String)> = sites
            .iter()
            .filter(|(_, line)| line.contains(arm))
            .collect();
        assert_eq!(
            sites.len(),
            1,
            "the `{arm}` mapping must exist once: {sites:?}"
        );
        assert!(
            sites[0].0.starts_with(AVAILABILITY_DERIVATION),
            "the `{arm}` mapping belongs to the availability impl: {sites:?}"
        );
        assert!(
            sites[0].1.contains(field),
            "the `{arm}` arm must read `{field}`: {sites:?}"
        );
    }

    // -- Check: no consumer carries per-command gate knowledge.
    assert!(
        leaks.is_empty(),
        "the palette builder, the drawer and the submit lane must stay \
         command-agnostic: {leaks:?}"
    );
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

/// Every site carrying one availability arm, as (`module:line`, trimmed line).
fn arm_sites(sources: &[(String, String)], arm: &str) -> Vec<(String, String)> {
    sources
        .iter()
        .flat_map(|(module, source)| {
            source
                .lines()
                .enumerate()
                .filter(|(_, line)| line.contains(arm))
                .map(|(index, line)| (format!("{module}:{}", index + 1), line.trim().to_string()))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Every consumer module that names a gated command, and the command it names:
/// the name is the per-command knowledge the availability impl alone may hold.
fn gate_leaks(sources: &[(String, String)]) -> Vec<String> {
    let mut leaks = Vec::new();
    for module in AVAILABILITY_CONSUMERS {
        let Some((_, source)) = sources.iter().find(|(name, _)| name == module) else {
            continue;
        };
        for command in GATED_COMMANDS {
            if source.contains(command) {
                leaks.push(format!("{module} names `{command}`"));
            }
        }
    }
    leaks
}

// endregion: --- Test Support
