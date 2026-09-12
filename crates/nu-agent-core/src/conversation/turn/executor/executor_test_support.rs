//! Shared fixtures and helpers for turn-executor tests.

use std::sync::Arc;

use super::super::test::{
    default_circuit_breaker, default_doom_state, default_last_total_tokens,
    default_output_repetition, default_repetition_guard,
};
use super::test_utils::{message_text, test_compaction_config};
use super::*;
use crate::bus::Bus;
use crate::config::Config;
use crate::conversation::managers::SessionManager;
use crate::conversation::state::memory::MemoryState;
use crate::session::{CachedMemory, FsSessionStore, SessionStore, StoreEntry};
use crate::tools::closure::ClosureRegistry;
use crate::tools::handler::McpToolRegistry;

pub(super) type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Build a `MemoryState<FsSessionStore>` backed by the given tempdir (no
/// compaction — `CachedMemory` is used directly).
pub(super) fn make_memory_state(temp_dir: &tempfile::TempDir) -> MemoryState<FsSessionStore> {
    let store = Arc::new(FsSessionStore::new(temp_dir.path().to_path_buf()));
    MemoryState::new(store)
}

/// Build a `ToolInfra` with deterministic defaults and the given bus.
pub(super) fn default_tool_infra(bus: Bus) -> ToolInfra {
    ToolInfra {
        closure_registry: Arc::new(ClosureRegistry::default()),
        mcp_registry: Arc::new(McpToolRegistry::empty()),
        tool_server_handle: rig::tool::server::ToolServer::new().run(),
        visible_tool_definitions: vec![],
        circuit_breaker: default_circuit_breaker(),
        doom_state: default_doom_state(),
        output_repetition: default_output_repetition(),
        repetition_guard: default_repetition_guard(),
        last_total_tokens: default_last_total_tokens(),
        bus,
    }
}

/// Build a `TurnExecutor` with a deterministic compaction config and the given
/// shared model handle.
pub(super) fn make_executor<'a, ST, S>(
    config: &'a Config,
    memory_state: &'a mut S,
    model: Arc<std::sync::Mutex<rig::agent::ModelHandle>>,
    tool_infra: ToolInfra,
) -> TurnExecutor<'a, S, ST>
where
    ST: SessionStore + Clone + Send + Sync + 'static,
    S: SessionManager<
            Memory = crate::conversation::state::memory::MemoryOf<ST>,
            InnerMemory = CachedMemory<ST>,
        >,
{
    TurnExecutor::new(
        config,
        memory_state,
        tool_infra,
        model,
        test_compaction_config(crate::bus::create_bus()),
    )
}

/// Shared repetition state for the multi-turn stop test: one Arc reused
/// across every `execute()` so the escalation ladder accumulates (the
/// executor resets it per RETRY attempt only, not per turn).
pub(super) fn shared_repetition_state()
-> Arc<std::sync::Mutex<crate::hook::output_repetition::RepetitionState>> {
    Arc::new(std::sync::Mutex::new(
        crate::hook::output_repetition::RepetitionState::default(),
    ))
}

/// Load all persisted messages for a session from the store.
pub(super) async fn load_persisted_messages(
    memory_state: &MemoryState<FsSessionStore>,
    session_id: &str,
) -> Result<Vec<crate::types::Message>> {
    let entries = memory_state
        .inner_memory()
        .load_all(session_id)
        .await
        .map_err(|e| format!("store load should succeed: {e:?}"))?;
    Ok(entries
        .iter()
        .filter_map(|e| match e {
            StoreEntry::Message(m) => Some(m.clone()),
            _ => None,
        })
        .collect())
}

/// Count messages whose first text content starts with the provider-feedback
/// prefix.
pub(super) fn feedback_message_count(messages: &[crate::types::Message]) -> usize {
    messages
        .iter()
        .filter(|m| {
            message_text(m).is_some_and(|t| {
                t.starts_with(crate::conversation::turn::feedback::FEEDBACK_PREFIX)
            })
        })
        .count()
}

/// Count messages whose first text content starts with the max-turns steering
/// prefix.
pub(super) fn max_turns_steering_message_count(messages: &[crate::types::Message]) -> usize {
    messages
        .iter()
        .filter(|m| {
            message_text(m).is_some_and(|t| {
                t.starts_with(crate::conversation::turn::feedback::MAX_TURNS_FEEDBACK_PREFIX)
            })
        })
        .count()
}

/// A no-op `log::Log` used to enable error logging in tests so the
/// `log::error!` macros in `TurnExecutor` actually evaluate their arguments —
/// including the preview slices under test.
pub(super) struct ErrorNoopLogger;

impl log::Log for ErrorNoopLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, _record: &log::Record) {}

    fn flush(&self) {}
}

static ERROR_LOGGER_INSTALL: std::sync::Once = std::sync::Once::new();

/// Install the no-op error logger exactly once per test binary. Without a
/// logger, `log::max_level()` is Off and `log::error!` skips its argument
/// evaluation, so the preview construction is never executed.
pub(super) fn install_error_logger() {
    ERROR_LOGGER_INSTALL.call_once(|| {
        log::set_boxed_logger(Box::new(ErrorNoopLogger)).ok();
        log::set_max_level(log::LevelFilter::Error);
    });
}

/// An error message whose `TurnError` Display form (the string the error-log
/// previews slice) is longer than 200 bytes with byte 200 inside a 2-byte
/// char. rig wraps `MockError::Provider(msg)` as
/// `CompletionError::ProviderError(msg)`, whose Display prefixes
/// `"ProviderError: "` (15 bytes), so the previewed msg is
/// `"ProviderError: " + message`. With 184 ASCII bytes + 'é' + suffix the
/// 2-byte 'é' lands at msg bytes 199..201, straddling byte 200. Classified as
/// `Unknown` (non-retryable, not model-correctable), so the turn fails fast
/// to the hard-error path.
pub(super) fn multibyte_error_message() -> String {
    format!("{}é rest of the provider error", "a".repeat(184))
}

/// Build a User message whose content is a single Text item.
pub(super) fn user_with_text(text: &str) -> crate::types::Message {
    crate::types::Message::user(text)
}
