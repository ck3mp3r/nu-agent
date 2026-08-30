use nu_agent_core::protocol::event::UiEvent;
use nu_agent_core::renderer::UiRenderer;

use super::StderrUiRenderer;
use crate::policy::{UiPolicy, Verbosity};

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
        text: "hello world".to_string(),
    });
    renderer.emit(&UiEvent::Completed { tool_calls: 0 });
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

    renderer.emit(&UiEvent::LlmStarted);
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
            verbosity: Verbosity::Normal,
        },
        false,
    );

    // First streaming sequence
    renderer.emit(&UiEvent::LlmStarted);
    renderer.emit(&UiEvent::AssistantMessage {
        text: "first".to_string(),
    });
    renderer.emit(&UiEvent::Completed { tool_calls: 0 });

    // Second streaming sequence - should start fresh
    renderer.emit(&UiEvent::LlmStarted);
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
            text: "streaming text".to_string(),
        });

        // Subsequent messages should keep streaming
        renderer.emit(&UiEvent::AssistantMessage {
            text: "streaming text continues".to_string(),
        });

        renderer.emit(&UiEvent::Completed { tool_calls: 0 });
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

    renderer.emit(&UiEvent::LlmStarted);
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

    renderer.emit(&UiEvent::LlmStarted);
    renderer.emit(&UiEvent::AssistantMessage {
        text: "trace output".to_string(),
    });
    renderer.emit(&UiEvent::Completed { tool_calls: 0 });
    renderer.flush();

    let stderr_out = String::from_utf8(stderr_bytes).expect("utf8");
    assert!(stderr_out.contains("trace output"));
}

#[test]
fn tool_display_renders_diff_with_stats() {
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Normal,
        },
        false,
    );
    let display = nu_agent_core::protocol::event::ToolDisplay {
        title: "edit file".to_string(),
        sections: vec![nu_agent_core::protocol::event::ToolDisplaySection {
            label: "diff".to_string(),
            language: "diff".to_string(),
            content: "+added\n-removed\nunchanged".to_string(),
            stats: Some(nu_agent_core::protocol::event::ToolDisplayStats {
                files_changed: Some(2),
                insertions: Some(3),
                deletions: Some(1),
                ..Default::default()
            }),
        }],
    };
    renderer.emit(&UiEvent::ToolCompleted {
        name: "edit".to_string(),
        source: "code".to_string(),
        arguments: "{}".to_string(),
        success: true,
        result: "ok".to_string(),
        display: Some(display),
        error_kind: None,
        message: None,
    });
    renderer.flush();
    let stderr_out = String::from_utf8(stderr_bytes).expect("utf8");
    assert!(stderr_out.contains("2 files changed"));
}

#[test]
fn llm_end_prints_token_usage() {
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
        text: "hi".to_string(),
    });
    renderer.emit(&UiEvent::LlmCompleted {
        response_chars: 2,
        tool_calls: 0,
        input_tokens: 80,
        output_tokens: 20,
        total_tokens: 100,
    });
    renderer.flush();
    let stderr_out = String::from_utf8(stderr_bytes).expect("utf8");
    assert!(stderr_out.contains("100 tokens"));
    assert!(stderr_out.contains("80 in + 20 out"));
}

#[test]
fn compaction_completed_uses_success_color() {
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Normal,
        },
        true,
    );
    renderer.emit(&UiEvent::CompactionCompleted {
        source: "auto".to_string(),
        summary_preview: "s".to_string(),
        summary_body: "s".to_string(),
    });
    renderer.flush();
    let stderr_out = String::from_utf8(stderr_bytes).expect("utf8");
    assert!(stderr_out.contains("\u{1b}[32m"));
}

#[test]
fn no_color_renders_compaction_plain() {
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Normal,
        },
        false,
    );
    renderer.emit(&UiEvent::CompactionCompleted {
        source: "auto".to_string(),
        summary_preview: "s".to_string(),
        summary_body: "s".to_string(),
    });
    renderer.flush();
    let stderr_out = String::from_utf8(stderr_bytes).expect("utf8");
    assert!(!stderr_out.contains("\u{1b}["));
}
