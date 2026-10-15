use super::*;

use crate::conversation::compaction::compactor::SummaryArtifact;

#[tokio::test]
async fn artifact_is_user_message_with_required_header_and_summary() -> Result<()> {
    let bus = Bus::default();
    let model = stream_model(&["decided to refactor the cache"]);
    let compactor = make_compactor(model, bus, None);

    let evicted = vec![Message::user("old message")];
    let artifact = compactor
        .compact("conv-1", &evicted, None, "test_source")
        .await
        .map_err(|e| format!("compact should succeed: {e:?}"))?;

    let msg: Message = artifact.into();
    assert!(
        matches!(msg, Message::User { .. }),
        "artifact must convert to Message::User, got {msg:?}"
    );
    assert_eq!(
        user_text(&msg),
        format!("{SUMMARY_HEADER}decided to refactor the cache")
    );
    Ok(())
}

#[tokio::test]
async fn carry_over_summary_is_prepended_into_summarizer_input() -> Result<()> {
    let bus = Bus::default();
    // Two scripted turns: the first produces the initial summary, the second
    // is compacted with that summary supplied as `carry_over`.
    let model = MockCompletionModel::from_stream_turns([
        [
            MockStreamEvent::Text("first summary".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        [
            MockStreamEvent::Text("second summary".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let compactor = make_compactor(model.clone(), bus, None);

    let evicted = vec![Message::user("second-batch of old messages")];
    let first_artifact = compactor
        .compact(
            "conv-1",
            &[Message::user("first-batch")],
            None,
            "test_source",
        )
        .await
        .map_err(|e| format!("first compact should succeed: {e:?}"))?;

    let _ = compactor
        .compact("conv-1", &evicted, Some(&first_artifact), "test_source")
        .await
        .map_err(|e| format!("second compact should succeed: {e:?}"))?;

    // The second (last) request's prompt must include the carry-over text.
    let prompt = last_request_prompt(&model)?;
    assert!(
        prompt.contains("first summary"),
        "carry-over summary missing from summarizer input, got: {prompt}"
    );
    Ok(())
}

#[tokio::test]
async fn started_and_summary_chunk_events_are_emitted_on_bus() -> Result<()> {
    let bus = Bus::default();
    let mut rx = bus.compaction().subscribe();
    let model = stream_model(&["chunk one", "chunk two"]);
    let compactor = make_compactor(model, bus, None);

    let evicted = vec![Message::user("old message")];
    compactor
        .compact("conv-1", &evicted, None, "test_source")
        .await
        .map_err(|e| format!("compact should succeed: {e:?}"))?;

    let started = rx.recv().await.map_err(|_| "Started event emitted")?;
    assert!(
        matches!(started, CompactionEvent::Started { .. }),
        "first event should be Started, got {started:?}"
    );

    let chunk = rx.recv().await.map_err(|_| "SummaryChunk event emitted")?;
    assert!(
        matches!(chunk, CompactionEvent::SummaryChunk { .. }),
        "expected a SummaryChunk after Started, got {chunk:?}"
    );
    Ok(())
}

#[tokio::test]
async fn single_message_conversation_produces_valid_summary() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let model = stream_model(&["one message summarized"]);
    let compactor = make_compactor(model.clone(), bus, None);
    let evicted = vec![Message::user("the only message")];

    // -- Exec
    let artifact = compactor
        .compact("conv-1", &evicted, None, "test_source")
        .await
        .map_err(|e| format!("compact should succeed: {e:?}"))?;

    // -- Check
    assert_eq!(artifact.summary(), "one message summarized");
    let msg: Message = artifact.into();
    assert_eq!(
        user_text(&msg),
        format!("{SUMMARY_HEADER}one message summarized")
    );
    let prompt = last_request_prompt(&model)?;
    assert!(
        prompt.contains("the only message"),
        "the single evicted message must reach the summarizer, got: {prompt}"
    );
    Ok(())
}

#[tokio::test]
async fn artifact_reports_provider_total_tokens() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("summary".to_string()),
        MockStreamEvent::final_response_with_total_tokens(42),
    ]]);
    let compactor = make_compactor(model, bus, None);

    // -- Exec
    let artifact = compactor
        .compact("conv-1", &[Message::user("old")], None, "test_source")
        .await
        .map_err(|e| format!("compact should succeed: {e:?}"))?;

    // -- Check
    assert_eq!(
        artifact.total_tokens(),
        Some(42),
        "the artifact must carry the provider's total token count"
    );
    Ok(())
}

#[test]
fn from_marker_summary_artifact_has_no_token_attribution() {
    // -- Setup & Fixtures
    let artifact = SummaryArtifact::from_marker_summary("cached summary");

    // -- Exec
    let msg: Message = artifact.clone().into();

    // -- Check
    assert_eq!(artifact.summary(), "cached summary");
    assert_eq!(artifact.total_tokens(), None);
    assert_eq!(user_text(&msg), format!("{SUMMARY_HEADER}cached summary"));
}

#[tokio::test]
async fn completed_event_preview_is_capped_at_two_hundred_chars() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let mut rx = bus.compaction().subscribe();
    let long_summary = "s".repeat(500);
    let model = stream_model(&[&long_summary]);
    let compactor = make_compactor(model, bus, None);

    // -- Exec
    compactor
        .compact("conv-1", &[Message::user("old")], None, "test_source")
        .await
        .map_err(|e| format!("compact should succeed: {e:?}"))?;

    // -- Check
    let mut completed = None;
    while let Ok(event) = rx.recv().await {
        if let CompactionEvent::Completed {
            summary_preview,
            summary_body,
            ..
        } = event
        {
            completed = Some((summary_preview, summary_body));
            break;
        }
    }
    let (preview, body) = completed.ok_or("a Completed event must be emitted")?;
    assert_eq!(
        preview.chars().count(),
        200,
        "the preview must be capped at 200 characters"
    );
    assert_eq!(
        body.chars().count(),
        500,
        "the body must carry the full summary"
    );
    Ok(())
}
