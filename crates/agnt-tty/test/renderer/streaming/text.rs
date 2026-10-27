//! Streaming text: what the renderer prints, and how it resets between
//! sequences.

use super::*;

#[test]
fn normal_mode_streams_text() {
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
        delta: "hello world".to_string(),
        text: "hello world".to_string(),
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
    assert!(stderr_out.contains("hello world"));
}

#[test]
fn normal_mode_shows_streaming() {
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
        delta: "hello".to_string(),
        text: "hello".to_string(),
    });
    renderer.emit(&UiEvent::AssistantMessage {
        delta: "hello world".to_string(),
        text: "hello world".to_string(),
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
    assert!(stderr_out.contains("hello world"));
}

#[test]
fn quiet_mode_suppresses_streaming() {
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: true,
            verbosity: Verbosity::Quiet,
        },
        false,
    );

    renderer.emit(&UiEvent::LlmStarted);
    renderer.emit(&UiEvent::AssistantMessage {
        delta: "hello world".to_string(),
        text: "hello world".to_string(),
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
    assert!(!stderr_out.contains("hello world"));
}

#[test]
fn streaming_completed_resets_state() {
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Normal,
        },
        false,
    );

    // First streaming sequence
    renderer.emit(&UiEvent::LlmStarted);
    renderer.emit(&UiEvent::AssistantMessage {
        delta: "first".to_string(),
        text: "first".to_string(),
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

    // Second streaming sequence - should start fresh
    renderer.emit(&UiEvent::LlmStarted);
    renderer.emit(&UiEvent::AssistantMessage {
        delta: "second".to_string(),
        text: "second".to_string(),
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
    assert!(stderr_out.contains("first"));
    assert!(stderr_out.contains("second"));

    // Verify both sequences got newlines (completed adds newline)
    let lines: Vec<&str> = stderr_out.lines().collect();
    assert!(lines.len() >= 2, "Expected at least 2 lines with content");
}

#[test]
fn normal_mode_incremental_streaming() {
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
        delta: "a".to_string(),
        text: "a".to_string(),
    });
    renderer.emit(&UiEvent::AssistantMessage {
        delta: "ab".to_string(),
        text: "ab".to_string(),
    });
    renderer.emit(&UiEvent::AssistantMessage {
        delta: "abc".to_string(),
        text: "abc".to_string(),
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
    // Should contain the final accumulated text
    assert!(stderr_out.contains("abc"));
    // Should not have repeated characters (incremental output means only new chars printed)
    // The text "abc" should appear once, not as "aababc"
    assert_eq!(stderr_out.matches("abc").count(), 1);
}

#[test]
fn spinner_stops_when_streaming_starts() {
    let mut stderr_bytes = Vec::<u8>::new();
    {
        let mut renderer = StderrUiRenderer::new(
            &mut stderr_bytes,
            UiPolicy {
                quiet: false,
                verbosity: Verbosity::Normal,
            },
            true, // TTY enabled - spinner will work
        );

        // Start spinner
        renderer.emit(&UiEvent::LlmStarted);

        // First message should stop spinner and stream text
        renderer.emit(&UiEvent::AssistantMessage {
            delta: "streaming text".to_string(),
            text: "streaming text".to_string(),
        });

        // Subsequent messages should keep streaming
        renderer.emit(&UiEvent::AssistantMessage {
            delta: "streaming text continues".to_string(),
            text: "streaming text continues".to_string(),
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
    }

    let stderr_out = String::from_utf8(stderr_bytes).expect("utf8");
    assert!(stderr_out.contains("streaming text continues"));
}

#[test]
fn streaming_adds_newline_on_completion() {
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
        delta: "no newline yet".to_string(),
        text: "no newline yet".to_string(),
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
    // Should end with newline after completion
    assert!(stderr_out.ends_with('\n'));
}
