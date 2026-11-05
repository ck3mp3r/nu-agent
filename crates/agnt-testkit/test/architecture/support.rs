//! Shared fixtures for the architecture drivers.

use std::fs;
use std::path::{Path, PathBuf};

pub type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Fixtures

/// The assertion ARCH-4, one type per distinction.
pub const ARCH_4: &str = "ARCH-4";

/// The assertion ARCH-8, components take channels, not the whole bus.
pub const ARCH_8: &str = "ARCH-8";

/// The assertion ARCH-11.a, a kernel identity is not publicly settable.
pub const ARCH_11A: &str = "ARCH-11.a";

/// The assertion ARCH-11.b, the kernel names no display type.
pub const ARCH_11B: &str = "ARCH-11.b";

/// The assertion ARCH-12, the kernel's dependency boundary.
pub const ARCH_12: &str = "ARCH-12";

/// The recorded baselines, one row per assertion.
///
/// An empty row is a property that holds everywhere in today's tree. A row that
/// is not empty is a counted defect: the assertion passes while its set of
/// violations equals the row exactly.
///
/// ARCH-11.a's row holds both records whose identity is publicly settable:
/// `ToolCall` publishes the id its constructor derived, so a caller can replace
/// an anchor with one the arguments do not produce, and `ToolCallLine` publishes
/// the key of the call it represents, so a row can be built for a call it did
/// not come from.
///
/// ARCH-4's row holds every `pub enum` whose name another `pub enum` also
/// carries: seven distinctions, sixteen declarations. Merging any pair is a
/// later slice's claim (`7e4f330c` drains `ToolErrorKind`), and its baseline
/// entry is removed with it.
///
/// ARCH-8's row holds every production site that takes or hands out the whole
/// bus: thirty-nine signatures and fields that a later slice narrows to the
/// channel each component actually uses.
pub const BASELINES: [(&str, &[&str]); 5] = [
    (
        ARCH_4,
        &[
            "ARCH-4 agnt-kernel/src/bus/cancel.rs:15: `CancelEvent` is declared 2 times in the workspace",
            "ARCH-4 agnt-core/src/bus/events.rs:3: `CancelEvent` is declared 2 times in the workspace",
            "ARCH-4 agnt-kernel/src/records.rs:356: `NoticeKind` is declared 2 times in the workspace",
            "ARCH-4 agnt-render/src/transcript/ir.rs:41: `NoticeKind` is declared 2 times in the workspace",
            "ARCH-4 agnt-kernel/src/records.rs:333: `PermissionDecision` is declared 3 times in the workspace",
            "ARCH-4 agnt-core/src/protocol/event.rs:2: `PermissionDecision` is declared 3 times in the workspace",
            "ARCH-4 agnt-core/src/hook/permission_resolver.rs:56: `PermissionDecision` is declared 3 times in the workspace",
            "ARCH-4 agnt-tui/src/state/app_state.rs:59: `ToolCallStatus` is declared 2 times in the workspace",
            "ARCH-4 agnt-storage/src/services/tool_calls.rs:20: `ToolCallStatus` is declared 2 times in the workspace",
            "ARCH-4 agnt-kernel/src/records.rs:221: `ToolErrorKind` is declared 2 times in the workspace",
            "ARCH-4 agnt-core/src/tools/handler/types.rs:17: `ToolErrorKind` is declared 2 times in the workspace",
            "ARCH-4 agnt-kernel/src/builtin.rs:44: `ToolKind` is declared 2 times in the workspace",
            "ARCH-4 agnt-render/src/transcript/entry.rs:18: `ToolKind` is declared 2 times in the workspace",
            "ARCH-4 agnt-core/src/orchestrator/turn_outcome.rs:21: `TurnOutcome` is declared 3 times in the workspace",
            "ARCH-4 agnt-core/src/conversation/turn/executor/core.rs:49: `TurnOutcome` is declared 3 times in the workspace",
            "ARCH-4 agnt-acp/src/session.rs:56: `TurnOutcome` is declared 3 times in the workspace",
        ],
    ),
    (
        ARCH_8,
        &[
            "ARCH-8 agnt-cli/src/mode/mod.rs:102: `&Bus` is a parameter",
            "ARCH-8 agnt-cli/src/mode/mod.rs:174: `&Bus` is a parameter",
            "ARCH-8 agnt-core/src/conversation/runtime.rs:219: `&Bus` is a parameter",
            "ARCH-8 agnt-core/src/hook/circuit_breaker_guard.rs:67: `&Bus` is a parameter",
            "ARCH-8 agnt-core/src/hook/permission_resolver.rs:136: `&Bus` is a parameter",
            "ARCH-8 agnt-core/src/hook/permission_resolver.rs:176: `&Bus` is a parameter",
            "ARCH-8 agnt-core/src/hook/permission_resolver.rs:221: `&Bus` is a parameter",
            "ARCH-8 agnt-core/src/hook/permission_resolver.rs:415: `&Bus` is a parameter",
            "ARCH-8 agnt-core/src/hooks/runner.rs:32: `&Bus` is a parameter",
            "ARCH-8 agnt-core/src/orchestrator/loop_impl.rs:284: `&Bus` is a parameter",
            "ARCH-8 agnt-core/src/orchestrator/router.rs:35: `&Bus` is a parameter",
            "ARCH-8 agnt-core/src/orchestrator/stages/context.rs:44: `&Bus` is a struct field",
            "ARCH-8 agnt-core/src/protocol/contracts.rs:212: `&Bus` is a parameter",
            "ARCH-8 agnt-tui/src/interactive.rs:288: `&Bus` is a parameter",
            "ARCH-8 agnt-acp/src/session.rs:108: `&Bus` is a return type",
            "ARCH-8 agnt-acp/src/translator.rs:248: `&Bus` is a parameter",
        ],
    ),
    (
        ARCH_11A,
        &[
            "ARCH-11.a agnt-kernel/src/records.rs:129: pub id: ToolCallId",
            "ARCH-11.a agnt-tui/src/state/app_state.rs:249: pub key: ToolCallId",
        ],
    ),
    (ARCH_11B, &[]),
    (ARCH_12, &[]),
];

/// The scratch directory fixture trees are built in, relative to this crate.
const FIXTURE_ROOT: &str = "../../tmp/architecture";

// endregion: --- Fixtures

// region:    --- Support

/// The baseline recorded for `assertion`, empty when none is.
pub fn baseline(assertion: &str) -> &'static [&'static str] {
    BASELINES
        .iter()
        .find(|(id, _)| *id == assertion)
        .map_or(&[], |(_, entries)| *entries)
}

/// An empty fixture workspace root named `name`, created fresh.
pub fn fixture_root(name: &str) -> Result<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(FIXTURE_ROOT)
        .join(name);
    if root.is_dir() {
        fs::remove_dir_all(&root)?;
    }
    fs::create_dir_all(&root)?;
    Ok(root)
}

/// Write `text` as the manifest of `krate` in the fixture workspace `root`.
pub fn write_manifest(root: &Path, krate: &str, text: &str) -> Result<()> {
    let dir = root.join("crates").join(krate);
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("Cargo.toml"), text)?;
    Ok(())
}

/// Write `text` as the source at `relative` inside `krate`'s `src` directory in
/// the fixture workspace `root`.
pub fn write_source(root: &Path, krate: &str, relative: &str, text: &str) -> Result<()> {
    let path = root.join("crates").join(krate).join("src").join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, text)?;
    Ok(())
}

// endregion: --- Support
