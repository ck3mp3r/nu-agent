use super::*;

// ---------------------------------------------------------------------------
// Gap 1A: wiremock diagnostics + close_open_tool_result_block integration test
// ---------------------------------------------------------------------------

#[tokio::test]
async fn journey_wiremock_basic_text_smoke() -> Result<()> {
    let mut h = JourneyHarness::new("journey-wiremock-smoke");
    let (server, client) = h.start_mock_server().await?;

    let sse_body = sse_text_response("hello from mock");
    {
        use wiremock::matchers::method;
        use wiremock::{Mock, ResponseTemplate};
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .append_header("content-type", "text/event-stream")
                    .set_body_bytes(sse_body.into_bytes()),
            )
            .mount(&server)
            .await;
    }

    let (r, _) = h.turn_with_client("test", &client, no_tools()).await?;
    assert!(r.is_ok(), "basic wiremock text turn must succeed: {r:?}");
    let msgs = h.raw_messages().await?;
    assert_eq!(msgs.len(), 2, "expect [user, assistant]; got: {msgs:?}");
    Ok(())
}

/// Turn 1: LLM calls nu__shell, tool executes (sub-turn 1 succeeds), sub-turn 2
/// returns HTTP 500 (server error). The executor must:
///   1. Persist [user(prompt), asst(tc1), user(tr1)] via inject_missing_tool_results
///   2. Append a synthetic assistant close-block message via close_open_tool_result_block
///   3. Return Err
///
/// Turn 2: Normal text response. Must succeed — proves the session is no longer broken
/// (the message history ends with Assistant, so the next User can follow without API error).
#[tokio::test]
async fn journey_hard_error_after_tool_results_session_remains_valid() -> Result<()> {
    let mut h = JourneyHarness::new("journey-gap1a");
    let (server, client) = h.start_mock_server().await?;

    // Turn 1: tool call succeeds, sub-turn 2 → HTTP 500
    let tool_call_body = sse_tool_call_response("tc1", "nu__shell", "{\"command\":\"git pull\"}");
    {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, ResponseTemplate};

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .append_header("content-type", "text/event-stream")
                    .set_body_bytes(tool_call_body.into_bytes()),
            )
            .up_to_n_times(1)
            .mount(&server)
            .await;

        // The 500 is up_to_n_times(1) so it's consumed by sub-turn 2 of turn 1
        // and does NOT interfere with turn 2's request.
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(500).set_body_bytes(
                b"{\"error\":{\"message\":\"server error\",\"type\":\"api_error\"}}".to_vec(),
            ))
            .up_to_n_times(1)
            .mount(&server)
            .await;
    }

    let (r1, _) = h
        .turn_with_client("run something", &client, nu_shell_tool("result_42"))
        .await?;
    assert!(r1.is_err(), "turn 1 must fail with server error");

    let msgs = h.raw_messages().await?;
    assert_eq!(
        msgs.len(),
        4,
        "expect [user, asst(tc1), user(tr1), asst(close)]"
    );
    // last message must be the synthetic assistant that closes the tool block
    assert!(
        matches!(&msgs[3], rig::message::Message::Assistant { .. }),
        "last message must be Assistant, got: {:?}",
        msgs[3]
    );

    // Turn 2: must succeed — proves the session is no longer broken
    let text_body = sse_text_response("recovered successfully");
    {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, ResponseTemplate};
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .append_header("content-type", "text/event-stream")
                    .set_body_bytes(text_body.into_bytes()),
            )
            .mount(&server)
            .await;
    }

    let (r2, _) = h.turn_with_client("continue", &client, no_tools()).await?;
    assert!(r2.is_ok(), "turn 2 must succeed after repair — was: {r2:?}");
    assert_eq!(h.raw_messages().await?.len(), 6);
    Ok(())
}

// ---------------------------------------------------------------------------
// Gap 2A: Token estimate warning
// ---------------------------------------------------------------------------

/// Verifies that a context warning is emitted when the estimated token count
/// of the session history exceeds the configured threshold before a turn.
#[tokio::test]
async fn journey_context_warning_emitted_near_limit() -> Result<()> {
    let config = crate::config::Config {
        model_context_tokens: Some(100),
        context_warning_threshold: Some(0.5), // warn at 50 tokens
        ..crate::config::Config::default()
    };
    let mut h = JourneyHarness::new_with_config("journey-ctx-warn", config);
    let (server, client) = h.start_mock_server().await?;

    // Pre-populate session with enough content to exceed threshold.
    // Execute a first turn to build up history.
    {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, ResponseTemplate};
        // Non-repeating filler content: identical 20-char delta chunks would
        // legitimately trip the output-repetition detector (5 identical deltas).
        let filler: String = (0..200)
            .map(|i| {
                format!(
                    "{}{}",
                    char::from_u32(0x4e00 + (i % 500) as u32).unwrap_or('x'),
                    i / 500
                )
            })
            .collect();
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .append_header("content-type", "text/event-stream")
                    .set_body_bytes(sse_text_response(&filler).into_bytes()),
            )
            .mount(&server)
            .await;
    }
    let _ = h
        .turn_with_client("initial prompt with some content here", &client, no_tools())
        .await?;

    // Execute a second turn — pre_turn_messages will include the first turn's history
    // which should exceed the 50-token threshold (100 * 0.5).
    {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, ResponseTemplate};
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .append_header("content-type", "text/event-stream")
                    .set_body_bytes(sse_text_response("done").into_bytes()),
            )
            .mount(&server)
            .await;
    }
    let (r2, events) = h.turn_with_client("follow up", &client, no_tools()).await?;

    assert!(r2.is_ok(), "turn must succeed even with warning: {r2:?}");
    let has_warning = events
        .iter()
        .any(|e| matches!(e, UiEvent::Warning { message } if message.contains("context window")));
    assert!(
        has_warning,
        "expected context window warning, got events: {events:?}"
    );
    Ok(())
}

/// Verifies that no context warning is emitted when `model_context_tokens` is `None`
/// (the default), regardless of session size.
#[tokio::test]
async fn journey_no_context_warning_when_not_configured() -> Result<()> {
    // Config::default() has model_context_tokens = None → no warning ever
    let mut h = JourneyHarness::new("journey-no-ctx-warn");
    let (server, client) = h.start_mock_server().await?;

    {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, ResponseTemplate};
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .append_header("content-type", "text/event-stream")
                    .set_body_bytes(sse_text_response("ok").into_bytes()),
            )
            .mount(&server)
            .await;
    }
    let (r, events) = h.turn_with_client("hello", &client, no_tools()).await?;

    assert!(r.is_ok());
    let has_ctx_warning = events
        .iter()
        .any(|e| matches!(e, UiEvent::Warning { message } if message.contains("context window")));
    assert!(
        !has_ctx_warning,
        "no context warning expected when model_context_tokens is None"
    );
    Ok(())
}
