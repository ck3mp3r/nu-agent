use nu_agent_core::policy::{UiPolicy, Verbosity};
use nu_agent_core::protocol::event::UiEvent;
use nu_agent_core::renderer::UiRenderer;

use crate::renderer::StderrUiRenderer;

#[test]
fn default_mode_suppresses_streaming() {
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Normal,
        },
        false,
    );

    renderer.emit(&UiEvent::LlmStart);
    renderer.emit(&UiEvent::AssistantMessage {
        text: "hello world".to_string(),
    });
    renderer.emit(&UiEvent::Completed { tool_calls: 0 });
    renderer.flush();

    let stderr_out = String::from_utf8(stderr_bytes).expect("utf8");
    assert!(!stderr_out.contains("hello world"));
}

#[test]
fn verbose_mode_shows_streaming() {
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Verbose,
        },
        false,
    );

    renderer.emit(&UiEvent::LlmStart);
    renderer.emit(&UiEvent::AssistantMessage {
        text: "hello".to_string(),
    });
    renderer.emit(&UiEvent::AssistantMessage {
        text: "hello world".to_string(),
    });
    renderer.emit(&UiEvent::Completed { tool_calls: 0 });
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

    renderer.emit(&UiEvent::LlmStart);
    renderer.emit(&UiEvent::AssistantMessage {
        text: "hello world".to_string(),
    });
    renderer.emit(&UiEvent::Completed { tool_calls: 0 });
    renderer.flush();

    let stderr_out = String::from_utf8(stderr_bytes).expect("utf8");
    assert!(!stderr_out.contains("hello world"));
}

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

    renderer.emit(&UiEvent::Warning {
        message: "test warning".to_string(),
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

    renderer.emit(&UiEvent::Warning {
        message: "test warning".to_string(),
    });
    renderer.flush();

    let stderr_out = String::from_utf8(stderr_bytes).expect("utf8");
    assert!(stderr_out.contains("test warning"));
}

#[test]
fn streaming_completed_resets_state() {
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Verbose,
        },
        false,
    );

    // First streaming sequence
    renderer.emit(&UiEvent::LlmStart);
    renderer.emit(&UiEvent::AssistantMessage {
        text: "first".to_string(),
    });
    renderer.emit(&UiEvent::Completed { tool_calls: 0 });

    // Second streaming sequence - should start fresh
    renderer.emit(&UiEvent::LlmStart);
    renderer.emit(&UiEvent::AssistantMessage {
        text: "second".to_string(),
    });
    renderer.emit(&UiEvent::Completed { tool_calls: 0 });
    renderer.flush();

    let stderr_out = String::from_utf8(stderr_bytes).expect("utf8");
    assert!(stderr_out.contains("first"));
    assert!(stderr_out.contains("second"));

    // Verify both sequences got newlines (completed adds newline)
    let lines: Vec<&str> = stderr_out.lines().collect();
    assert!(lines.len() >= 2, "Expected at least 2 lines with content");
}

#[test]
fn verbose_mode_incremental_streaming() {
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Verbose,
        },
        false,
    );

    renderer.emit(&UiEvent::LlmStart);
    renderer.emit(&UiEvent::AssistantMessage {
        text: "a".to_string(),
    });
    renderer.emit(&UiEvent::AssistantMessage {
        text: "ab".to_string(),
    });
    renderer.emit(&UiEvent::AssistantMessage {
        text: "abc".to_string(),
    });
    renderer.emit(&UiEvent::Completed { tool_calls: 0 });
    renderer.flush();

    let stderr_out = String::from_utf8(stderr_bytes).expect("utf8");
    // Should contain the final accumulated text
    assert!(stderr_out.contains("abc"));
    // Should not have repeated characters (incremental output means only new chars printed)
    // The text "abc" should appear once, not as "aababc"
    assert_eq!(stderr_out.matches("abc").count(), 1);
}

#[test]
fn spinner_stops_when_streaming_starts_in_verbose_mode() {
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Verbose,
        },
        true, // TTY enabled - spinner will work
    );

    // Start spinner
    renderer.emit(&UiEvent::LlmStart);
    assert!(renderer.spinner_active_for_test());

    // First message should stop spinner
    renderer.emit(&UiEvent::AssistantMessage {
        text: "streaming text".to_string(),
    });
    assert!(!renderer.spinner_active_for_test());

    // Subsequent messages should keep spinner stopped
    renderer.emit(&UiEvent::AssistantMessage {
        text: "streaming text continues".to_string(),
    });
    assert!(!renderer.spinner_active_for_test());

    renderer.emit(&UiEvent::Completed { tool_calls: 0 });
    renderer.flush();

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
            verbosity: Verbosity::Verbose,
        },
        false,
    );

    renderer.emit(&UiEvent::LlmStart);
    renderer.emit(&UiEvent::AssistantMessage {
        text: "no newline yet".to_string(),
    });
    renderer.emit(&UiEvent::Completed { tool_calls: 0 });
    renderer.flush();

    let stderr_out = String::from_utf8(stderr_bytes).expect("utf8");
    // Should end with newline after completion
    assert!(stderr_out.ends_with('\n'));
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

    renderer.emit(&UiEvent::LlmStart);
    renderer.emit(&UiEvent::AssistantMessage {
        text: "response text".to_string(),
    });
    renderer.emit(&UiEvent::Warning {
        message: "test warning".to_string(),
    });
    renderer.emit(&UiEvent::Completed { tool_calls: 0 });
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

    renderer.emit(&UiEvent::LlmStart);
    renderer.emit(&UiEvent::AssistantMessage {
        text: "trace output".to_string(),
    });
    renderer.emit(&UiEvent::Completed { tool_calls: 0 });
    renderer.flush();

    let stderr_out = String::from_utf8(stderr_bytes).expect("utf8");
    assert!(stderr_out.contains("trace output"));
}
