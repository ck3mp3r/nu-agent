use super::*;

// ---------------------------------------------------------------------------
// Scenario: multi-byte UTF-8 tool output through the trace-log preview
// ---------------------------------------------------------------------------

/// Regression: `HookChain::on_tool_result` builds a trace-log preview via
/// `&result_text[..2000]`. When byte 2000 falls inside a multi-byte UTF-8 char
/// and trace logging is enabled, the slice panicked and killed the turn. The
/// full-turn path is the only way to reach that code: `log` macros skip
/// argument evaluation without an installed logger, and `HookContext` has no
/// public constructor, so the preview can only execute inside a real rig run.
#[tokio::test]
async fn journey_tool_result_with_multibyte_utf8_at_byte_2000_does_not_panic() -> Result<()> {
    // -- Setup & Fixtures
    install_trace_logger();
    let mut h = JourneyHarness::new("journey-trace-preview-multibyte");

    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc1", "nu__shell", serde_json::json!({})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::Text("done".into()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);

    // -- Exec & Check
    // Byte 2000 of the tool result lands inside the 2-byte 'é'; before the
    // fix the trace preview sliced &result_text[..2000] and panicked here.
    let (r, _) = h
        .turn("run it", model, nu_shell_multibyte_tool().await)
        .await;
    assert!(
        r.is_ok(),
        "turn with multi-byte tool output must not panic: {r:?}"
    );

    let msgs = h.raw_messages().await?;
    assert_eq!(msgs.len(), 4, "expected 4 messages, got: {msgs:?}");
    assert_tool_result_in_msg(&msgs[2], "tc1", "\u{e9} rest of the output")?;
    Ok(())
}
