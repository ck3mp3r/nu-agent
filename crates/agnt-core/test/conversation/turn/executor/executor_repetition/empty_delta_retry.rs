use super::*;

/// A repetition stop whose stopped attempt reports an EMPTY delta must still
/// persist the stopped attempt before the steering retry, and the placeholder
/// must carry the ORIGINAL prompt, not the steered one.
///
/// The store's dangling user tail merges with the prompt into one user
/// message in rig's canonical history (`repair` merges adjacent non-empty
/// user messages by extending content, with no separator between), and the
/// anchor reads a message's text blocks concatenated. The merged text
/// neither is the prompt nor ends with the `"\n\n{prompt}"` steering suffix,
/// so the content anchor is absent and the fallback pair persists: the
/// original prompt and the interrupted placeholder.
///
/// The steered retry then completes, so the store ends with five messages: the
/// seed, the original prompt, the placeholder, the steered prompt and the
/// retry's answer.
#[tokio::test]
async fn repetition_stop_empty_delta_retry_persists_original_prompt_placeholder() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-repetition-empty-delta";
    let mut memory_state = make_memory_state(&temp_dir);

    // A dangling user tail: rig's repair merges it with the prompt, so the
    // stopped attempt's canonical history has the same length as the store and
    // its delta is empty.
    seed_rig_store(
        &memory_state,
        session_id,
        vec![crate::types::Message::user("dangling question")],
    )?;

    // Turn 1 stops intra-stream at the text delta (its final_response is never
    // consumed); turn 2 is the steered retry and completes.
    let stop_turn = vec![
        MockStreamEvent::Text("I will do the thing. ".repeat(8)),
        MockStreamEvent::final_response_with_total_tokens(1),
    ];
    let retry_turn = vec![
        MockStreamEvent::Text("I changed my mind.".to_string()),
        MockStreamEvent::final_response_with_default_usage(),
    ];
    let model = MockCompletionModel::from_stream_turns(vec![stop_turn, retry_turn]);
    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(crate::bus::create_bus()),
    );

    // -- Exec
    let outcome = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await
        .map_err(|e| format!("the steered retry must complete the turn: {e:?}"))?;

    // -- Check
    assert!(
        matches!(outcome, TurnOutcome::Completed),
        "the steered retry must complete the turn; got {outcome:?}"
    );
    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert_eq!(
        persisted.len(),
        5,
        "the seed, the empty-delta placeholder pair and the retry's exchange; got {persisted:?}"
    );
    assert_eq!(
        message_text(&persisted[0]).as_deref(),
        Some("dangling question"),
        "the seed must stay untouched; got {persisted:?}"
    );
    assert_eq!(
        message_text(&persisted[1]).as_deref(),
        Some("hello"),
        "the placeholder's user message must carry the ORIGINAL prompt, not the steered one; got {persisted:?}"
    );
    assert_eq!(
        message_text(&persisted[2]).as_deref(),
        Some("[interrupted — turn stopped before responding]"),
        "an empty delta must persist the interrupted placeholder; got {persisted:?}"
    );
    let steered = message_text(&persisted[3]).ok_or("the steered prompt must carry text")?;
    assert!(
        steered.starts_with(OUTPUT_REPETITION_BACKOFF_MESSAGE),
        "the retry's prompt must carry the repetition backoff message; got {steered:?}"
    );
    assert!(
        steered.ends_with("hello"),
        "the retry's prompt must keep the original prompt; got {steered:?}"
    );
    assert_eq!(
        message_text(&persisted[4]).as_deref(),
        Some("I changed my mind."),
        "the retry's answer must be persisted; got {persisted:?}"
    );

    Ok(())
}
