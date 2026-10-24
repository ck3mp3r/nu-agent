//! Retry-persistence tests: a retry persists the failed attempt's delta before
//! it re-runs, so the next attempt loads the work the previous one committed.

use super::*;

/// A backoff retry persists the failed attempt's delta before re-running, so
/// the store holds both attempts' deltas in order: the failed attempt's prompt,
/// then the retry's prompt and reply.
#[tokio::test]
async fn backoff_retry_persists_the_failed_attempt_delta() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config {
        max_retries: Some(3),
        retry_base_delay_ms: Some(1),
        ..Config::default()
    };
    let mut memory_state = MemoryState::new(RigStoreConfig::in_memory());
    let session_id = "test-recover-backoff-retry";
    // Turn 1: retryable provider error. Turn 2: success.
    let model = MockCompletionModel::from_stream_turns([
        vec![MockStreamEvent::error(
            "500 api_error internal server error",
        )],
        vec![
            MockStreamEvent::Text("recovered".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);

    // -- Exec
    let outcome = {
        let mut executor = TurnExecutor::new(
            &config,
            &mut memory_state,
            tool_infra(crate::bus::create_bus()),
            shared_handle(model),
            test_compaction_config(crate::bus::create_bus()),
        );
        executor
            .execute(
                ExecuteInput {
                    prompt: "hello".to_string(),
                    preamble: None,
                    span: nu_protocol::Span::test_data(),
                },
                MockResolver(PermissionDecision::Allow),
                Some(session_id),
            )
            .await
            .map_err(|e| format!("the backoff retry should recover the turn: {e:?}"))?
    };

    // -- Check
    assert!(
        matches!(outcome, TurnOutcome::Completed),
        "the backoff retry must complete the turn"
    );
    let messages = stored(&memory_state, session_id)?;
    assert_eq!(
        messages.len(),
        3,
        "the failed attempt's delta, then the retry's prompt and reply; got {messages:?}"
    );
    assert_eq!(messages[0], Message::user("hello"));
    assert_eq!(messages[1], Message::user("hello"));
    let Message::Assistant(assistant) = &messages[2] else {
        return Err("the retry's reply must be persisted as an assistant message".into());
    };
    assert!(
        matches!(&assistant.content[0], AssistantContent::Text(text) if text.text == "recovered"),
        "the retry's reply is persisted; got {messages:?}"
    );
    Ok(())
}
