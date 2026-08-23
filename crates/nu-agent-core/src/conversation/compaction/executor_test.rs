//! GREEN tests for CompactionExecutor API surface.
//!
//! These tests verify the CompactionExecutor struct can be constructed and exposes
//! the expected API. They replace the Phase B RED stubs.

use super::*;
use crate::bus::CompactionEvent;
use crate::compaction::CompactionParams;
use crate::config::Config;
use crate::conversation::providers::CachedProviderClient;
use crate::protocol::event::UiEvent;
use crate::session::{CachedMemory, FsSessionStore};
use crate::types::Message;
use rig::memory::ConversationMemory;
use rig::test_utils::{MockCompletionModel, MockStreamEvent};
use std::sync::Arc;

#[derive(Default)]
struct TestProgressUi;

impl ProgressUi for TestProgressUi {
    fn emit(&mut self, _event: &UiEvent) {}
    fn flush(&mut self) {}
    fn take_cancel_requested(&self) -> bool {
        false
    }
}

/// Helper to build a minimal Config for testing.
fn test_config() -> Config {
    Config {
        a2a_port: None,
        provider: "copilot".to_string(),
        provider_impl: None,
        model: "gpt-4o".to_string(),
        api_key: None,
        base_url: None,
        preamble: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tokens: None,
        max_tool_turns: None,
        temperature: None,
        read_timeout_secs: None,
        max_tool_result_bytes: None,
        model_context_tokens: None,
        context_warning_threshold: None,
        max_retries: None,
        retry_base_delay_ms: None,
        max_tool_calls_per_subturn: None,
        additional_params: None,
        a2a_enabled: None,
        session_store_type: None,
    }
}

#[test]
fn compaction_executor_new_constructs_without_panic() {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let store = Arc::new(FsSessionStore::new(temp_dir.path().to_path_buf()));
    let memory = CachedMemory::<FsSessionStore>::new(Arc::clone(&store));

    let _executor = CompactionExecutor::new(
        &config,
        &memory,
        "test-session",
        CompactionParams::default(),
        crate::bus::create_bus(),
    );
    // Construction succeeded — no panic.
}

/// Seed `count` alternating user/assistant messages into memory.
async fn seed_messages(memory: &CachedMemory<FsSessionStore>, session_id: &str, count: usize) {
    let mut messages = Vec::new();
    for i in 0..count {
        if i % 2 == 0 {
            messages.push(Message::user(format!("msg{i}")));
        } else {
            messages.push(Message::assistant(format!("msg{i}")));
        }
    }
    memory.append(session_id, messages).await.unwrap();
}

fn make_bus() -> (
    crate::bus::Bus,
    tokio::sync::broadcast::Receiver<CompactionEvent>,
) {
    let bus = crate::bus::create_bus();
    let rx = bus.compaction().subscribe();
    (bus, rx)
}

fn summarize_stream() -> MockCompletionModel {
    MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("summary text".to_string()),
        MockStreamEvent::FinalResponse(rig::test_utils::MockResponse::new()),
    ]])
}

#[tokio::test]
async fn execute_ok_none_sends_no_started_event() {
    let temp_dir = tempfile::tempdir().unwrap();
    let store = Arc::new(FsSessionStore::new(temp_dir.path().to_path_buf()));
    let memory = CachedMemory::<FsSessionStore>::new(store);
    let session_id = "test-session";
    // Fewer than keep_recent (10) messages → compact() early-returns Ok(None).
    seed_messages(&memory, session_id, 3).await;

    let (bus, mut rx) = make_bus();
    let config = test_config();
    let executor = CompactionExecutor::new(
        &config,
        &memory,
        session_id,
        CompactionParams::default(),
        bus,
    );
    let model = summarize_stream();
    let cached_client = CachedProviderClient::Mock(model);
    let mut ui = TestProgressUi;

    let result = executor
        .execute(
            &mut ui,
            CompactionTriggerSource::SlashCompact,
            &cached_client,
        )
        .await;

    assert_eq!(result.unwrap(), None);
    // No CompactionEvent::Started should be on the bus.
    tokio::time::timeout(std::time::Duration::from_millis(50), rx.recv())
        .await
        .expect_err("no compaction event should be emitted for Ok(None)");
}

#[tokio::test]
async fn err_sends_started_before_failed() {
    let temp_dir = tempfile::tempdir().unwrap();
    let store = Arc::new(FsSessionStore::new(temp_dir.path().to_path_buf()));
    let memory = CachedMemory::<FsSessionStore>::new(Arc::clone(&store));
    let session_id = "test-session";
    // Enough messages so compaction actually runs and hits the failing model.
    seed_messages(&memory, session_id, 20).await;

    let (bus, mut rx) = make_bus();
    let config = test_config();
    let executor = CompactionExecutor::new(
        &config,
        &memory,
        session_id,
        CompactionParams::default(),
        bus,
    );
    let model = MockCompletionModel::from_stream_turns([[MockStreamEvent::error("boom")]]);
    let cached_client = CachedProviderClient::Mock(model);
    let mut ui = TestProgressUi;

    let result = executor
        .execute(
            &mut ui,
            CompactionTriggerSource::SlashCompact,
            &cached_client,
        )
        .await;
    assert!(result.is_err());

    // Started fires before the summarizer LLM call; Failed closes the block.
    let started = rx.recv().await.expect("Started event");
    assert!(
        matches!(started, CompactionEvent::Started { .. }),
        "expected Started first, got {started:?}"
    );
    let failed = rx.recv().await.expect("Failed event");
    assert!(
        matches!(failed, CompactionEvent::Failed { .. }),
        "expected Failed after Started, got {failed:?}"
    );
}

#[tokio::test]
async fn ok_some_sends_started_before_triggered() {
    let temp_dir = tempfile::tempdir().unwrap();
    let store = Arc::new(FsSessionStore::new(temp_dir.path().to_path_buf()));
    let memory = CachedMemory::<FsSessionStore>::new(store);
    let session_id = "test-session";
    seed_messages(&memory, session_id, 20).await;

    let (bus, mut rx) = make_bus();
    let config = test_config();
    let executor = CompactionExecutor::new(
        &config,
        &memory,
        session_id,
        CompactionParams::default(),
        bus,
    );
    let model = summarize_stream();
    let cached_client = CachedProviderClient::Mock(model);
    let mut ui = TestProgressUi;

    let result = executor
        .execute(
            &mut ui,
            CompactionTriggerSource::SlashCompact,
            &cached_client,
        )
        .await;
    assert!(result.unwrap().is_some());

    // Started must be the first event, preceding the streaming SummaryChunk
    // and the final Triggered.
    let first = rx.recv().await.expect("compact event");
    assert!(
        matches!(first, CompactionEvent::Started { .. }),
        "expected Started first, got {first:?}"
    );
    // Drain SummaryChunk events until Triggered arrives.
    loop {
        let event = rx.recv().await.expect("compact event");
        match event {
            CompactionEvent::SummaryChunk { .. } => continue,
            CompactionEvent::Triggered { .. } => break,
            other => panic!("unexpected event before Triggered: {other:?}"),
        }
    }
}
