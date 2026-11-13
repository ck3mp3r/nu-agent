//! Shared fixtures for the architecture drivers.

use std::fs;
use std::path::{Path, PathBuf};

pub type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Fixtures

/// The assertion ARCH-2, the two vocabularies stay in their own crates.
pub const ARCH_2: &str = "ARCH-2";

/// The assertion ARCH-4, one type per distinction.
pub const ARCH_4: &str = "ARCH-4";

/// The assertion ARCH-5, no test double in production source.
pub const ARCH_5: &str = "ARCH-5";

/// The assertion ARCH-8, components take channels, not the whole bus.
pub const ARCH_8: &str = "ARCH-8";

/// The assertion ARCH-6, every declared public item has a consumer.
pub const ARCH_6: &str = "ARCH-6";

/// The ARCH-6 per-crate allowance rows, one file per crate.
pub use super::baselines_arch6::ARCH_6_CRATES;

/// The ARCH-6 per-crate impl-member rows recorded so far, one file per crate.
pub use super::baselines_arch6::ARCH_6_MEMBER_CRATES;

/// The assertion ARCH-9.a, the aggregate runtime exposes no public field.
pub const ARCH_9A: &str = "ARCH-9.a";

/// The assertion ARCH-9.b, construction sits at its recorded homes.
pub const ARCH_9B: &str = "ARCH-9.b";

/// The assertion ARCH-10, no consumer names the aggregate runtime.
pub const ARCH_10: &str = "ARCH-10";

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
/// ARCH-11.a's row is empty: every identity field is private and adopted at
/// construction, so no record's identity can be replaced from outside.
///
/// ARCH-4's row holds every `pub enum` whose name another `pub enum` also
/// carries: six distinctions, twelve declarations. Merging any pair is a later
/// slice's claim, and its baseline entry is removed with it.
///
/// Two of those rows are ruled parallel vocabulary, not a defect: the two
/// `RenderInput` drawer types (`ui_renderer` draws 3 arms, the TUI reducer 4)
/// repeat a name and share no knowledge. Each drawer owns its own vocabulary
/// because the channel sets differ per drawer; a shared type would move one
/// drawer's knowledge into the other, and cross-surface reuse is forbidden.
///
/// ARCH-8's row is the 17-site ratchet: every production site outside the
/// composition root that takes or hands out the whole bus is recorded, and the
/// row only ever shrinks.
///
/// ARCH-5's row holds the one rig test double a production source still names:
/// the `CachedProviderClient::Mock` payload, which is how the fabric injects a
/// stub provider at the LLM wire. ARCH-5 permits that mock, so the row is an
/// allowance, not a defect: the entry leaves the baseline only with its code.
pub const BASELINES: [(&str, &[&str]); 10] = [
    (ARCH_2, &[]),
    (
        ARCH_5,
        &[
            "ARCH-5 agnt-runtime/src/conversation/providers.rs:344: `rig::test_utils::MockCompletionModel` is a rig test double",
        ],
    ),
    (
        ARCH_4,
        &[
            "ARCH-4 agnt-kernel/src/records.rs:450: `NoticeKind` is declared 2 times in the workspace",
            "ARCH-4 agnt-render/src/transcript/ir.rs:41: `NoticeKind` is declared 2 times in the workspace",
            "ARCH-4 agnt-kernel/src/records.rs:416: `PermissionDecision` is declared 2 times in the workspace",
            "ARCH-4 agnt-runtime/src/hook/permission_resolver.rs:58: `PermissionDecision` is declared 2 times in the workspace",
            "ARCH-4 agnt-tui/src/state/app_state/types.rs:33: `ToolCallStatus` is declared 2 times in the workspace",
            "ARCH-4 agnt-storage/src/services/tool_calls.rs:20: `ToolCallStatus` is declared 2 times in the workspace",
            "ARCH-4 agnt-kernel/src/builtin.rs:44: `ToolKind` is declared 2 times in the workspace",
            "ARCH-4 agnt-render/src/transcript/entry.rs:18: `ToolKind` is declared 2 times in the workspace",
            "ARCH-4 agnt-tty/src/renderer/ui_renderer.rs:8: `RenderInput` is declared 2 times in the workspace",
            "ARCH-4 agnt-tui/src/interaction/reducer/fold.rs:25: `RenderInput` is declared 2 times in the workspace",
            "ARCH-4 agnt-kernel/src/records.rs:541: `TurnOutcome` is declared 2 times in the workspace",
            "ARCH-4 agnt-runtime/src/orchestrator/turn_outcome.rs:12: `TurnOutcome` is declared 2 times in the workspace",
        ],
    ),
    (
        ARCH_8,
        &[
            "ARCH-8 agnt-acp/src/session.rs:90: `Bus` is a struct field",
            "ARCH-8 agnt-runtime/src/conversation/builder.rs:535: `Bus` is a struct field",
            "ARCH-8 agnt-runtime/src/conversation/compaction/compactor.rs:142: `Bus` is a struct field",
            "ARCH-8 agnt-runtime/src/conversation/compaction/compactor.rs:162: `Bus` is a parameter",
            "ARCH-8 agnt-runtime/src/conversation/compaction/compactor.rs:182: `Bus` is a parameter",
            "ARCH-8 agnt-runtime/src/conversation/contracts.rs:27: `Bus` is a return type",
            "ARCH-8 agnt-runtime/src/conversation/runtime_build.rs:59: `Bus` is a struct field",
            "ARCH-8 agnt-runtime/src/conversation/turn/execute.rs:398: `Bus` is a struct field",
            "ARCH-8 agnt-runtime/src/conversation/turn/execute.rs:657: `Bus` is a parameter",
            "ARCH-8 agnt-runtime/src/conversation/turn/executor/core.rs:109: `Bus` is a struct field",
            "ARCH-8 agnt-runtime/src/conversation/turn/proxy.rs:18: `Bus` is a struct field",
            "ARCH-8 agnt-runtime/src/hook/chain/mod.rs:67: `Bus` is a struct field",
            "ARCH-8 agnt-runtime/src/hook/chain/mod.rs:93: `Bus` is a struct field",
            "ARCH-8 agnt-runtime/src/orchestrator/config.rs:125: `Bus` is a parameter",
            "ARCH-8 agnt-runtime/src/orchestrator/config.rs:211: `Bus` is a struct field",
            "ARCH-8 agnt-runtime/src/orchestrator/config.rs:34: `Bus` is a struct field",
            "ARCH-8 agnt-tui/src/interactive.rs:191: `Bus` is a parameter",
        ],
    ),
    (ARCH_11A, &[]),
    (ARCH_9A, &[]),
    (
        ARCH_9B,
        &[
            "ARCH-9.b agnt-cli/src/dispatch/a2a.rs:165: reqwest::Client::builder",
            "ARCH-9.b agnt-cli/src/main.rs:309: rustls::crypto::ring::default_provider::install_default",
            "ARCH-9.b agnt-plugin/src/main.rs:9: rustls::crypto::ring::default_provider::install_default",
            "ARCH-9.b agnt-runtime/src/config/models_cache.rs:109: reqwest::blocking::Client::new",
            "ARCH-9.b agnt-runtime/src/conversation/providers.rs:30: ensure_crypto_provider",
            "ARCH-9.b agnt-runtime/src/conversation/providers.rs:32: reqwest::Client::builder",
            "ARCH-9.b agnt-runtime/src/tools/handler/http.rs:62: reqwest::Client::builder",
            "ARCH-9.b agnt-runtime/src/tools/mcp/runtime.rs:263: ensure_crypto_provider",
            "ARCH-9.b agnt-runtime/src/tools/mcp/runtime.rs:264: reqwest::Client::builder",
            "ARCH-9.b agnt-runtime/src/utils/crypto.rs:11: rustls::crypto::ring::default_provider::install_default",
        ],
    ),
    (
        ARCH_10,
        &[
            "ARCH-10 agnt-acp/src/runtime.rs:16: AgentConversationRuntime",
            "ARCH-10 agnt-acp/src/runtime.rs:186: AgentConversationRuntime",
            "ARCH-10 agnt-acp/src/session.rs:134: AgentConversationRuntime",
            "ARCH-10 agnt-acp/src/session.rs:154: AgentConversationRuntime",
            "ARCH-10 agnt-acp/src/session.rs:161: AgentConversationRuntime",
            "ARCH-10 agnt-acp/src/session.rs:27: AgentConversationRuntime",
            "ARCH-10 agnt-acp/src/session.rs:778: AgentConversationRuntime",
            "ARCH-10 agnt-acp/src/session.rs:789: AgentConversationRuntime",
            "ARCH-10 agnt-acp/src/session.rs:91: AgentConversationRuntime",
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
