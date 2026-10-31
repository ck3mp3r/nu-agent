use super::*;

// ---------------------------------------------------------------------------
// Gap 1A: mock-server diagnostics + a failed turn saves nothing
// ---------------------------------------------------------------------------

#[tokio::test]
async fn journey_wiremock_basic_text_smoke() -> Result<()> {
    let mut h = JourneyHarness::new("journey-wiremock-smoke");
    let (server, client) = h.start_mock_server().await?;

    agnt_testkit::mount_sse_response(
        &server,
        "POST",
        None,
        agnt_testkit::sse_text_body("hello from mock"),
    )
    .await;

    let (r, _) = h.turn_with_client("test", &client, no_tools()).await?;
    assert!(r.is_ok(), "basic wiremock text turn must succeed: {r:?}");
    let msgs = h.raw_messages().await?;
    assert_eq!(msgs.len(), 2, "expect [user, assistant]; got: {msgs:?}");
    Ok(())
}

/// Turn 1: LLM calls nu__shell, tool executes (sub-turn 1 succeeds), sub-turn 2
/// returns HTTP 500 (server error). rig saves nothing for a failed run, so the
/// host persists the delta: the prompt, the tool call and its result.
///
/// Turn 2: Normal text response. Must succeed and save its own exchange.
#[tokio::test]
async fn journey_hard_error_after_tool_results_session_remains_valid() -> Result<()> {
    let mut h = JourneyHarness::new("journey-gap1a");
    let (server, client) = h.start_mock_server().await?;

    // Turn 1: tool call succeeds, sub-turn 2 → HTTP 500
    let tool_call_body =
        agnt_testkit::sse_tool_call_body("tc1", "nu__shell", "{\"command\":\"git pull\"}");
    {
        use agnt_testkit::matchers::{method, path};
        use agnt_testkit::{Mock, ResponseTemplate};

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
        .turn_with_client("run something", &client, nu_shell_tool("result_42")?)
        .await?;
    assert!(r1.is_err(), "turn 1 must fail with server error");

    // The failed turn's delta is persisted: the prompt, the tool call and its
    // result.
    let msgs = h.raw_messages().await?;
    assert_eq!(
        msgs.len(),
        3,
        "a failed turn persists its delta; got: {msgs:?}"
    );
    assert_user_text(&msgs[0], "run something");

    // Turn 2: must succeed and save its own exchange
    agnt_testkit::mount_sse_response(
        &server,
        "POST",
        Some("/chat/completions"),
        agnt_testkit::sse_text_body("recovered successfully"),
    )
    .await;

    let (r2, _) = h.turn_with_client("continue", &client, no_tools()).await?;
    assert!(r2.is_ok(), "turn 2 must succeed after repair — was: {r2:?}");
    assert_eq!(h.raw_messages().await?.len(), 5);
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

    // Seed the rig store with enough content to exceed the threshold. The
    // token estimate reads the conversation the agent loads, which is the rig
    // store.
    //
    // Non-repeating filler content: identical 20-char chunks would
    // legitimately trip the output-repetition detector.
    let filler: String = (0..200)
        .map(|i| {
            format!(
                "{}{}",
                char::from_u32(0x4e00 + (i % 500) as u32).unwrap_or('x'),
                i / 500
            )
        })
        .collect();
    h.seed_history(vec![
        Message::user("initial prompt with some content here"),
        Message::assistant(filler),
    ])?;

    // Execute a turn — the seeded history exceeds the 50-token threshold
    // (100 * 0.5).
    agnt_testkit::mount_sse_response(
        &server,
        "POST",
        Some("/chat/completions"),
        agnt_testkit::sse_text_body("done"),
    )
    .await;
    let (r2, events) = h.turn_with_client("follow up", &client, no_tools()).await?;

    assert!(r2.is_ok(), "turn must succeed even with warning: {r2:?}");
    let has_warning = events.iter().any(
        |e| matches!(e, SystemEvent::SystemMessage { kind: SystemKind::Warning, text } if text.contains("context window")),
    );
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

    agnt_testkit::mount_sse_response(
        &server,
        "POST",
        Some("/chat/completions"),
        agnt_testkit::sse_text_body("ok"),
    )
    .await;
    let (r, events) = h.turn_with_client("hello", &client, no_tools()).await?;

    assert!(r.is_ok());
    let has_ctx_warning = events.iter().any(
        |e| matches!(e, SystemEvent::SystemMessage { kind: SystemKind::Warning, text } if text.contains("context window")),
    );
    assert!(
        !has_ctx_warning,
        "no context warning expected when model_context_tokens is None"
    );
    Ok(())
}
