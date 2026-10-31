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

    renderer.emit(&SystemEvent::InferenceStarted);
    renderer.emit(&SystemEvent::InferenceChunk {
        delta: "provisional".to_string(),
        aggregated: "provisional".to_string(),
    });
    renderer.emit(&SystemEvent::InferenceStopped {
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

    renderer.emit(&SystemEvent::InferenceStarted);
    renderer.emit(&SystemEvent::InferenceChunk {
        delta: "provisional".to_string(),
        aggregated: "provisional".to_string(),
    });
    assert_eq!(
        renderer.streaming_printed_len,
        "provisional".len(),
        "the provisional chunk advanced the printed length"
    );

    renderer.emit(&SystemEvent::InferenceStopped {
        reason: String::new(),
    });

    // The stop resets the streaming state, so the retry is not sliced against
    // the discarded provisional length.
    assert_eq!(
        renderer.streaming_printed_len, 0,
        "the stop must reset the printed length"
    );

    // The retry's text is shorter than the discarded provisional text; without
    // the streaming reset it would be sliced away entirely.
    renderer.emit(&SystemEvent::InferenceChunk {
        delta: "retry".to_string(),
        aggregated: "retry".to_string(),
    });
    assert_eq!(
        renderer.streaming_printed_len,
        "retry".len(),
        "the retry prints from its first byte"
    );
    renderer.emit(&SystemEvent::TurnCompleted {
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
