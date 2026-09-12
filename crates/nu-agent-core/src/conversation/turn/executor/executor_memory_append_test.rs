//! Memory-append failure warning surfacing tests.

use std::sync::Arc;

use rig::test_utils::{MockCompletionModel, MockStreamEvent};

use super::executor_test_support::*;
use super::test_utils::{MockResolver, test_config};
use super::*;
use crate::conversation::state::memory::MemoryState;
use crate::session::{SessionStore, StoreEntry};
use crate::types::Message;

// ---------------------------------------------------------------------------
// Memory-append failure surfacing
// ---------------------------------------------------------------------------

/// A `SessionStore` whose `append` always fails, used to verify that failed
/// session-memory appends on turn error paths are surfaced (not silently
/// dropped). `create` succeeds so pre-population (the first write, which
/// routes to `create`) works and the turn's later append routes to the
/// failing `append`.
#[derive(Clone)]
struct FailingAppendStore;

#[derive(Debug)]
struct AppendError;

impl std::fmt::Display for AppendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "append exploded")
    }
}

impl std::error::Error for AppendError {}

impl SessionStore for FailingAppendStore {
    type Error = AppendError;

    async fn create(
        &self,
        _id: &str,
        _first_messages: &[Message],
    ) -> std::result::Result<(), Self::Error> {
        Ok(())
    }

    async fn load(
        &self,
        _id: &str,
    ) -> std::result::Result<Option<(crate::session::SessionMetadata, Vec<StoreEntry>)>, Self::Error>
    {
        Ok(None)
    }

    async fn append(
        &self,
        _id: &str,
        _entries: &[StoreEntry],
    ) -> std::result::Result<(), Self::Error> {
        Err(AppendError)
    }

    async fn replace_entries(
        &self,
        _id: &str,
        _entries: &[StoreEntry],
    ) -> std::result::Result<(), Self::Error> {
        Ok(())
    }

    async fn list(&self) -> std::result::Result<Vec<crate::session::SessionInfo>, Self::Error> {
        Ok(Vec::new())
    }

    async fn delete(&self, _id: &str) -> std::result::Result<(), Self::Error> {
        Ok(())
    }
}

/// A failed session-memory append on the hard-error path must be surfaced as
/// `WarningEvent::Message` on the bus warning channel (not silently dropped).
#[tokio::test]
async fn hard_error_with_failing_append_store_emits_warning_event() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let session_id = "test-failing-append-hard-error";
    let mut memory_state = MemoryState::new(Arc::new(FailingAppendStore));

    // First write routes to store.create (succeeds) and marks the session as
    // persisted, so the turn's later append routes to the failing store.append.
    {
        use rig::memory::ConversationMemory;
        memory_state
            .inner_memory()
            .append(session_id, vec![user_with_text("prior work")])
            .await
            .map_err(|e| format!("pre-populate append should succeed: {e:?}"))?;
    }

    let bus = crate::bus::create_bus();
    let mut warning_rx = bus.warning().subscribe();

    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::error("provider unavailable")]]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(bus),
    );

    // -- Exec
    let result = executor
        .execute(
            ExecuteInput {
                prompt: "new prompt".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // -- Check
    assert!(result.is_err(), "hard error must propagate as Err");

    let event = warning_rx
        .try_recv()
        .map_err(|e| format!("warning event should arrive after failed append: {e:?}"))?;
    match event {
        crate::bus::WarningEvent::Message { message } => {
            assert!(
                message.contains("hard error"),
                "warning must name the append site; got: {message}"
            );
        }
        other => {
            return Err(format!("expected WarningEvent::Message; got {other:?}").into());
        }
    }

    Ok(())
}

/// A succeeding session-memory append on the hard-error path must NOT emit a
/// warning event.
#[tokio::test]
async fn hard_error_with_working_store_emits_no_warning_event() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-working-append-hard-error";
    let mut memory_state = make_memory_state(&temp_dir);

    // Pre-populate so the turn's append is a second write (routes to
    // store.append) and succeeds against the working store.
    {
        use rig::memory::ConversationMemory;
        memory_state
            .inner_memory()
            .append(session_id, vec![user_with_text("prior work")])
            .await
            .map_err(|e| format!("pre-populate append should succeed: {e:?}"))?;
    }

    let bus = crate::bus::create_bus();
    let mut warning_rx = bus.warning().subscribe();

    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::error("provider unavailable")]]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(bus),
    );

    // -- Exec
    let result = executor
        .execute(
            ExecuteInput {
                prompt: "new prompt".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // -- Check
    assert!(result.is_err(), "hard error must propagate as Err");

    match warning_rx.try_recv() {
        Err(crate::bus::TryRecvError::Empty) => {} // no warning emitted — correct
        Err(other) => {
            return Err(format!("warning channel should stay open: {other:?}").into());
        }
        Ok(event) => {
            return Err(
                format!("successful append must not emit a warning event; got {event:?}").into(),
            );
        }
    }

    Ok(())
}
