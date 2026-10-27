//! Stop notices: the silent discard of provisional output and the streaming
//! reset it performs.

use super::*;

#[test]
fn stopped_with_empty_reason_prints_no_notice() {
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Normal,
        },
        false,
    );

    renderer.emit(&UiEvent::LlmStarted);
    renderer.emit(&UiEvent::AssistantMessage {
        delta: "provisional".to_string(),
        text: "provisional".to_string(),
    });
    renderer.emit(&UiEvent::Stopped {
        reason: String::new(),
    });
    renderer.flush();

    let stderr_out = String::from_utf8(stderr_bytes).expect("utf8");
    assert!(!stderr_out.contains("stopped:"));
}

#[test]
fn stopped_with_empty_reason_resets_streaming_for_retry() {
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Normal,
        },
        false,
    );

    renderer.emit(&UiEvent::LlmStarted);
    renderer.emit(&UiEvent::AssistantMessage {
        delta: "provisional".to_string(),
        text: "provisional".to_string(),
    });
    renderer.emit(&UiEvent::Stopped {
        reason: String::new(),
    });
    // The retry's text is shorter than the discarded provisional text; without
    // the streaming reset it would be sliced away entirely.
    renderer.emit(&UiEvent::AssistantMessage {
        delta: "retry".to_string(),
        text: "retry".to_string(),
    });
    renderer.emit(&UiEvent::Completed {
        tool_calls: 0,
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: None,
        cost: None,
        cost_complete: true,
        duration_ms: 0,
    });
    renderer.flush();

    let stderr_out = String::from_utf8(stderr_bytes).expect("utf8");
    assert!(stderr_out.contains("retry"));
}
