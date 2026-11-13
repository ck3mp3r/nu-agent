use super::*;

// ---------------------------------------------------------------------------
// Completed turn: rig writes JSONL via memory.append() — no explicit store write
// ---------------------------------------------------------------------------

/// After a successful (non-cancelled) turn, the rig-memory store holds the
/// conversation — the executor writes nothing itself.
///
/// This verifies the double-write elimination: executor.rs no longer appends
/// for completed turns. The single write happens inside rig's save at run end.
#[tokio::test]
async fn completed_turn_no_explicit_store_append_needed() -> Result<()> {
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-completed-session";
    let mut memory_state = make_memory_state(&temp_dir);

    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("Hello from LLM!".to_string()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(crate::bus::Bus::default()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
                task_id: None,
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // Collect response data before dropping the executor (which holds a mutable borrow)
    let response_data = executor.take_response_data();

    // Turn must complete normally
    assert!(
        result.is_ok(),
        "execute() must succeed; got: {:?}",
        result.err()
    );
    let outcome = result.map_err(|e| format!("completed turn must be Ok: {e:?}"))?;
    assert!(
        matches!(outcome, ExecuteOutcome::Completed),
        "completed turn must return ExecuteOutcome::Completed"
    );

    // rig saved the conversation through the rig-memory store — the executor
    // writes nothing itself.
    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert_eq!(
        persisted.len(),
        2,
        "completed turn: the rig store holds the prompt and the reply; got {persisted:?}"
    );

    // Response data must be available
    assert!(
        response_data.is_some(),
        "TurnResponseData must be populated after completed turn"
    );

    Ok(())
}
