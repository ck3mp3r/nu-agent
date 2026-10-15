use super::*;

// ---------------------------------------------------------------------------
// Gap 3: Retry on server error recovers
// ---------------------------------------------------------------------------

/// Integration test using a mock server: first POST → 500 server error (retryable),
/// second POST → success. Verifies the retry loop transparently recovers.
#[tokio::test]
async fn journey_retry_on_server_error_recovers() -> Result<()> {
    let config = crate::config::Config {
        max_retries: Some(3),
        retry_base_delay_ms: Some(1), // minimal delay for fast tests
        ..crate::config::Config::default()
    };
    let mut h = JourneyHarness::new_with_config("journey-retry-recover", config);
    let (server, client) = h.start_mock_server().await?;

    agnt_testkit::mount_error_then_sse(
        &server,
        "/chat/completions",
        500,
        "{\"error\":{\"message\":\"500 api_error internal server error\",\"type\":\"api_error\"}}",
        agnt_testkit::sse_text_body("recovered from 500"),
    )
    .await;

    let (r, _) = h
        .turn_with_client("test retry", &client, no_tools())
        .await?;
    assert!(
        r.is_ok(),
        "retry should recover from server error; got: {r:?}"
    );

    let msgs = h.raw_messages().await?;
    assert_eq!(
        msgs.len(),
        2,
        "expect [user, assistant] after successful retry; got {msgs:?}"
    );
    Ok(())
}
