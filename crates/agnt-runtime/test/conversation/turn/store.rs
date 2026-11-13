//! The turn's store opens: a refusal fails the turn, any other failure degrades.
//!
//! The fake memory answers each failure class the turn distinguishes, so no
//! database is opened: a refusing turn fails before its run, and a degrading one
//! runs against rig's mock completion model.

use std::sync::{Arc, Mutex};

use agnt_storage::StorageError;
use rig::test_utils::{MockCompletionModel, MockStreamEvent};

use super::super::*;
use crate::bus::Bus;
use crate::config::Config;
use crate::conversation::compaction::CompactionConfig;
use crate::conversation::compaction::compactor::NuCompactor;
use crate::conversation::state::memory::MemoryOf;
use crate::hook::agent_hook::DoomLoopState;
use crate::hook::output_repetition::RepetitionState;
use crate::hook::permission_resolver::{AsyncPermissionResolver, PermissionDecision};
use crate::session::settle::FactStore;
use crate::session::{CachedMemory, MemoryError, RigStoreConfig};
use crate::tools::closure::ClosureRegistry;
use crate::tools::handler::McpToolRegistry;
use crate::tools::mcp::circuit_breaker::McpCircuitBreaker;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Fixtures

/// The failure the fake memory answers every store open with.
#[derive(Clone, Copy)]
enum Failure {
    /// The database was migrated by a build that knows a version this one does
    /// not.
    NewerSchema,
    /// Any other backend failure: the class the turn degrades past.
    Backend,
}

impl Failure {
    /// The error this class answers with.
    fn error(self) -> MemoryError {
        match self {
            Self::NewerSchema => MemoryError::backend(StorageError::Migrate(
                sqlx::migrate::MigrateError::VersionMissing(7),
            )),
            Self::Backend => MemoryError::backend(std::io::Error::other("the log is gone")),
        }
    }
}

/// A session memory whose store and fact-table opens all fail with `failure`.
///
/// The hook's memory is a real in-memory one, and is never opened: a refusing
/// turn fails before its run, and a degrading one runs without a store.
struct RefusingMemory(Failure);

impl TurnMemory for RefusingMemory {
    fn memory(&self) -> MemoryOf {
        Arc::new(CachedMemory::new(RigStoreConfig::in_memory()))
    }

    fn rig_store(
        &self,
        _session_id: &str,
    ) -> impl std::future::Future<
        Output = core::result::Result<Arc<dyn rig_memory::Store>, MemoryError>,
    > + Send {
        let failure = self.0;
        async move { Err(failure.error()) }
    }

    fn facts(
        &self,
    ) -> impl std::future::Future<Output = core::result::Result<FactStore, MemoryError>> + Send
    {
        let failure = self.0;
        async move { Err(failure.error()) }
    }
}

/// A resolver that allows every tool call.
#[derive(Clone)]
struct Allow;

impl AsyncPermissionResolver for Allow {
    async fn resolve(
        &self,
        _tool_name: &str,
        _arguments: &str,
        _tool_call_id: Option<String>,
        _stream: &crate::bus::Stream,
        _facts: &crate::bus::FactsTx,
        _cancel: &crate::bus::CancelTx,
    ) -> PermissionDecision {
        PermissionDecision::Allow
    }
}

/// A model that answers the turn under test with one text reply.
fn model() -> MockCompletionModel {
    MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("reply".to_string()),
        MockStreamEvent::final_response_with_default_usage(),
    ]])
}

/// Compaction machinery that never triggers: no threshold and no model spec.
fn never_compacting(bus: Bus) -> CompactionConfig {
    CompactionConfig {
        compactor: NuCompactor::new(
            model().erase(),
            bus,
            rig_memory::CompactionPolicy::default(),
            None,
        ),
        reserve_pct: crate::config::defaults::DEFAULT_RESERVE_PCT,
    }
}

/// The tool infrastructure a turn reaches through, with no tool registered.
fn tool_infra(bus: Bus) -> executor::ToolInfra {
    executor::ToolInfra {
        store_latch: Default::default(),
        pending_summary: Default::default(),
        closure_registry: Arc::new(ClosureRegistry::default()),
        mcp_registry: Arc::new(McpToolRegistry::empty()),
        tool_server_handle: rig::tool::server::ToolServer::new().run(),
        visible_tool_definitions: vec![],
        circuit_breaker: Arc::new(Mutex::new(McpCircuitBreaker::default())),
        doom_state: Arc::new(Mutex::new(DoomLoopState::default())),
        output_repetition: Arc::new(Mutex::new(RepetitionState::default())),
        repetition_guard: true,
        meter: crate::hook::AnchorMeter::default(),
        turn_usage: crate::hook::TurnUsage::default(),
        session_usage: crate::hook::TurnUsage::default(),
        bus,
        cwd: std::path::PathBuf::from("/tmp"),
    }
}

/// A turn context over `memory`, for a session-backed turn when `session_backed`.
fn context<'a, M: TurnMemory>(
    memory: M,
    session_backed: bool,
    config: &'a Config,
) -> TurnContext<'a, M> {
    let bus = Bus::default();
    TurnContext::new(
        TurnConversation {
            memory,
            conversation_id: "test-store-refusal".to_string(),
            agent: String::new(),
            has_session: session_backed,
            dyn_model: Arc::new(Mutex::new(model().erase())),
            compaction: never_compacting(bus.clone()),
        },
        TurnInput {
            prompt: "Hello".to_string(),
            preamble: None,
            max_turns: None,
        },
        tool_infra(bus),
        config,
    )
}

// endregion: --- Fixtures

/// WHEN the store's migrations cannot carry the database, THEN THE execute_turn
/// function SHALL fail the turn on the refusal text.
#[tokio::test]
async fn test_turn_store_newer_schema_fails_the_turn() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config::default();
    let expected = Failure::NewerSchema.error().to_string();
    let ctx = context(RefusingMemory(Failure::NewerSchema), true, &config);

    // -- Exec
    let result = execute_turn(ctx, Allow).await;

    // -- Check
    let Err((error, _)) = result else {
        return Err("a refused store must fail the turn, not run it".into());
    };
    assert!(
        matches!(error, TurnError::StoreRefused { .. }),
        "the turn fails on the store refusal; got {error:?}"
    );
    assert_eq!(
        error.to_string(),
        expected,
        "the failure carries the refusal text"
    );
    assert_eq!(
        expected,
        "Memory backend error: migration 7 was previously applied but is missing in the \
         resolved migrations"
    );
    Ok(())
}

/// WHEN a store open fails with any other class, THEN THE execute_turn function
/// SHALL log a warn and run the turn to completion.
#[tokio::test]
async fn test_turn_store_other_backend_failure_degrades_the_turn() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config::default();
    let ctx = context(RefusingMemory(Failure::Backend), true, &config);

    // -- Exec
    let result = execute_turn(ctx, Allow).await;

    // -- Check
    let turn = result.map_err(|(error, _)| {
        format!("a non-refusal store failure must not fail the turn; got {error:?}")
    })?;
    assert!(!turn.cancelled, "the degraded turn runs to completion");
    assert_eq!(turn.text, "reply", "the turn answers from its model");
    Ok(())
}

/// WHEN a turn is not session-backed, THEN THE execute_turn function SHALL open
/// no store and record no facts.
#[tokio::test]
async fn test_turn_store_session_less_turn_opens_no_store() -> Result<()> {
    // -- Setup & Fixtures: a memory that would refuse, on a turn that never asks
    let config = Config::default();
    let ctx = context(RefusingMemory(Failure::NewerSchema), false, &config);

    // -- Exec
    let result = execute_turn(ctx, Allow).await;

    // -- Check
    let turn = result
        .map_err(|(error, _)| format!("a session-less turn opens no store; got {error:?}"))?;
    assert_eq!(turn.text, "reply", "the turn answers from its model");
    Ok(())
}
