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
use crate::session::{CachedMemory, RigStoreConfig};
use crate::tools::closure::ClosureRegistry;
use crate::tools::handler::McpToolRegistry;
use crate::tools::handler::builtin_tool::ToolRenderRegistry;

pub(super) type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Build a `MemoryState` over an in-memory rig store (no compaction —
/// `CachedMemory` is used directly).
pub(super) fn make_memory_state(_temp_dir: &tempfile::TempDir) -> MemoryState {
    MemoryState::new(RigStoreConfig::in_memory())
}

/// Build a `MemoryState` whose rig store is a SQLite database under
/// `temp_dir`, so separate `MemoryState`s share one conversation.
pub(super) fn make_memory_state_on_disk(temp_dir: &tempfile::TempDir) -> MemoryState {
    MemoryState::new(
        RigStoreConfig::new(crate::session::StoreType::Sqlite).with_cache_dir(temp_dir.path()),
    )
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
        render_registry: ToolRenderRegistry::default(),
        cwd: std::path::PathBuf::from("/tmp"),
    }
}

/// Build a `TurnExecutor` with a deterministic compaction config and the given
/// shared model.
pub(super) fn make_executor<'a, S>(
    config: &'a Config,
    memory_state: &'a mut S,
    model: Arc<std::sync::Mutex<rig::DynModel<rig::operation::Completion>>>,
    tool_infra: ToolInfra,
) -> TurnExecutor<'a, S>
where
    S: SessionManager<
            Memory = crate::conversation::state::memory::MemoryOf,
            InnerMemory = CachedMemory,
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

/// The conversation the rig-memory store holds for a session.
///
/// The old session store is frozen after S2b, so conversation assertions read
/// the rig store, which is what the agent writes.
pub(super) async fn load_persisted_messages(
    memory_state: &MemoryState,
    session_id: &str,
) -> Result<Vec<crate::types::Message>> {
    let store = memory_state
        .inner_memory()
        .rig_store(session_id)
        .map_err(|e| format!("rig store should open: {e:?}"))?;
    let conversation = rig_memory::load(&*store, session_id)
        .map_err(|e| format!("rig store load should succeed: {e:?}"))?;
    Ok(conversation
        .messages()
        .iter()
        .map(|message| (**message).clone())
        .collect())
}

/// Seed the rig-memory store of a session with `messages`.
pub(super) fn seed_rig_store(
    memory_state: &MemoryState,
    session_id: &str,
    messages: Vec<crate::types::Message>,
) -> Result<()> {
    let store = memory_state
        .inner_memory()
        .rig_store(session_id)
        .map_err(|e| format!("rig store should open: {e:?}"))?;
    let mut conversation = rig_memory::load(&*store, session_id)
        .map_err(|e| format!("rig store load should succeed: {e:?}"))?;
    conversation.extend(messages);
    rig_memory::save(&*store, session_id, &mut conversation)
        .map_err(|e| format!("rig store save should succeed: {e:?}"))?;
    Ok(())
}

/// The raw bytes of a session's rig log.
pub(super) fn rig_log(memory_state: &MemoryState, session_id: &str) -> Result<Vec<u8>> {
    let store = memory_state
        .inner_memory()
        .rig_store(session_id)
        .map_err(|e| format!("rig store should open: {e:?}"))?;
    rig_memory::Store::read_log(&*store, session_id)
        .map_err(|e| format!("rig log should read: {e:?}").into())
}

/// The prompt text of a captured request: the last text part of the last
/// message.
///
/// `CompletionRequest::chat_history`'s very last message carries the prompt as
/// its last text part, so a steering message prepended to the prompt shows up
/// here. rig merges a prompt into a trailing user message, so the prompt is the
/// last part of that message, not the first.
pub(super) fn request_prompt(request: &rig::completion::request::CompletionRequest) -> String {
    request
        .chat_history
        .last()
        .and_then(|message| match &**message {
            crate::types::Message::User { content } => {
                content.iter().rev().find_map(|item| match item {
                    crate::types::UserContent::Text(text) => Some(text.text.clone()),
                    _ => None,
                })
            }
            other => message_text(other),
        })
        .unwrap_or_default()
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
