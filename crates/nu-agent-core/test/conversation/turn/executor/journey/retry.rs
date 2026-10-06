use super::*;

// ---------------------------------------------------------------------------
// Gap 3: Retry on server error recovers
// ---------------------------------------------------------------------------

/// Integration test using wiremock: first POST → 500 server error (retryable),
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

    let sse_body = sse_text_response("recovered from 500");
    {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, ResponseTemplate};

        // First request: 500 server error
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(500).set_body_bytes(
                b"{\"error\":{\"message\":\"500 api_error internal server error\",\"type\":\"api_error\"}}".to_vec(),
            ))
            .up_to_n_times(1)
            .mount(&server)
            .await;

        // Second request: success
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .append_header("content-type", "text/event-stream")
                    .set_body_bytes(sse_body.into_bytes()),
            )
            .mount(&server)
            .await;
    }

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
