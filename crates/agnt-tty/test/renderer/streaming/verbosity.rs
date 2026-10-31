//! Verbosity gating: which levels show warnings, and which show streaming.

use super::*;

#[test]
fn warning_requires_very_verbose() {
    // With Verbose, warnings should not appear
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Verbose,
        },
        false,
    );

    renderer.emit(&SystemEvent::SystemMessage {
        kind: SystemKind::Warning,
        text: "test warning".to_string(),
    });
    renderer.flush();

    let stderr_out = String::from_utf8(stderr_bytes).expect("utf8");
    assert!(!stderr_out.contains("test warning"));

    // With VeryVerbose, warnings should appear
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::VeryVerbose,
        },
        false,
    );

    renderer.emit(&SystemEvent::SystemMessage {
        kind: SystemKind::Warning,
        text: "test warning".to_string(),
    });
    renderer.flush();

    let stderr_out = String::from_utf8(stderr_bytes).expect("utf8");
    assert!(stderr_out.contains("test warning"));
}

#[test]
fn very_verbose_shows_both_warnings_and_streaming() {
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::VeryVerbose,
        },
        false,
    );

    renderer.emit(&SystemEvent::InferenceStarted);
    renderer.emit(&SystemEvent::InferenceChunk {
        delta: "response text".to_string(),
        aggregated: "response text".to_string(),
    });
    renderer.emit(&SystemEvent::SystemMessage {
        kind: SystemKind::Warning,
        text: "test warning".to_string(),
    });
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
    assert!(stderr_out.contains("response text"));
    assert!(stderr_out.contains("test warning"));
}

#[test]
fn trace_mode_shows_streaming() {
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Trace,
        },
        false,
    );

    renderer.emit(&SystemEvent::InferenceStarted);
    renderer.emit(&SystemEvent::InferenceChunk {
        delta: "trace output".to_string(),
        aggregated: "trace output".to_string(),
    });
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
    assert!(stderr_out.contains("trace output"));
}
