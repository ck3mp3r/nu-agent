use std::sync::{Arc, Mutex};

use super::StderrUiRenderer;
use nu_agent_core::policy::{UiPolicy, Verbosity};
use nu_agent_core::protocol::event::UiEvent;
use nu_agent_core::renderer::UiRenderer;

#[derive(Clone)]
struct FakeRenderer {
    events: Arc<Mutex<Vec<String>>>,
}

impl FakeRenderer {
    fn new(_policy: UiPolicy) -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn events(&self) -> Vec<String> {
        self.events.lock().expect("events").clone()
    }
}

impl UiRenderer for FakeRenderer {
    fn emit(&mut self, event: &UiEvent) {
        let label = match event {
            UiEvent::LlmStarted => "llm_start",
            UiEvent::Tick => "tick",
            UiEvent::LlmCompleted { .. } => "llm_end",
            UiEvent::ToolStarted { .. } => "tool_start",
            UiEvent::ToolCompleted { .. } => "tool_end",
            UiEvent::PermissionRequested { .. } => "permission_requested",
            UiEvent::PermissionDecisionSubmitted { .. } => "permission_decision_submitted",
            UiEvent::PermissionDecisionTimedOut { .. } => "permission_decision_timed_out",
            UiEvent::PermissionDecisionIgnored { .. } => "permission_decision_ignored",
            UiEvent::Warning { .. } => "warning",
            UiEvent::TurnError { .. } => "turn_error",
            UiEvent::CompactionStarted { .. } => "compaction_started",
            UiEvent::CompactionSummaryChunk { .. } => "compaction_summary_chunk",
            UiEvent::CompactionCompleted { .. } => "compaction_completed",
            UiEvent::CompactionFailed { .. } => "compaction_failed",
            UiEvent::AssistantMessage { .. } => "assistant_message",
            UiEvent::Completed { .. } => "completed",
        };
        self.events.lock().expect("events").push(label.to_string());
    }

    fn flush(&mut self) {}
}

fn run_mock_flow<R: UiRenderer>(renderer: &mut R) {
    renderer.emit(&UiEvent::LlmStarted);
    renderer.emit(&UiEvent::Tick);
    renderer.emit(&UiEvent::LlmCompleted {
        response_chars: 3,
        tool_calls: 1,
        input_tokens: 2,
        output_tokens: 1,
        total_tokens: 3,
    });
    renderer.emit(&UiEvent::ToolStarted {
        name: "t".to_string(),
        source: "closure".to_string(),
        arguments: "{}".to_string(),
    });
    renderer.emit(&UiEvent::ToolCompleted {
        name: "t".to_string(),
        source: "closure".to_string(),
        arguments: "{}".to_string(),
        success: true,
        result: "ok".to_string(),
        display: None,
        error_kind: None,
        message: None,
    });
    renderer.emit(&UiEvent::Completed { tool_calls: 1 });
    renderer.flush();
}

#[test]
fn renderer_is_substitutable_via_trait_boundary() {
    let mut fake = FakeRenderer::new(UiPolicy {
        quiet: false,
        verbosity: Verbosity::Normal,
    });
    run_mock_flow(&mut fake);
    assert_eq!(
        fake.events(),
        vec![
            "llm_start",
            "tick",
            "llm_end",
            "tool_start",
            "tool_end",
            "completed"
        ]
    );
}

#[test]
fn stderr_renderer_writes_only_to_stderr_sink() {
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
    renderer.emit(&UiEvent::Tick);
    renderer.emit(&UiEvent::LlmCompleted {
        response_chars: 5,
        tool_calls: 0,
        input_tokens: 3,
        output_tokens: 2,
        total_tokens: 5,
    });
    renderer.flush();

    let stderr_out = String::from_utf8(stderr_bytes).expect("utf8");
    assert!(stderr_out.trim().is_empty());
}

#[test]
fn quiet_mode_suppresses_non_essential_progress_and_warnings() {
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: true,
            verbosity: Verbosity::Quiet,
        },
        true,
    );

    renderer.emit(&UiEvent::LlmStarted);
    renderer.emit(&UiEvent::Tick);
    renderer.emit(&UiEvent::Completed { tool_calls: 0 });
    renderer.emit(&UiEvent::Warning {
        message: "essential warning".to_string(),
    });
    renderer.flush();

    let stderr_out = String::from_utf8(stderr_bytes).expect("utf8");
    assert!(!stderr_out.contains("thinking"));
    assert!(!stderr_out.contains("completed"));
    assert!(!stderr_out.contains("essential warning"));
}

#[test]
fn verbose_mode_shows_warnings() {
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::VeryVerbose,
        },
        true,
    );

    renderer.emit(&UiEvent::Warning {
        message: "important warning".to_string(),
    });
    renderer.flush();

    let stderr_out = String::from_utf8(stderr_bytes).expect("utf8");
    assert!(stderr_out.contains("important warning"));
}

#[test]
fn default_busy_flow_uses_spinner_without_redundant_persistent_busy_lines() {
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Normal,
        },
        true,
    );

    renderer.emit(&UiEvent::LlmStarted);
    renderer.emit(&UiEvent::Tick);
    renderer.emit(&UiEvent::LlmCompleted {
        response_chars: 42,
        tool_calls: 0,
        input_tokens: 30,
        output_tokens: 12,
        total_tokens: 42,
    });
    renderer.flush();

    let stderr_out = String::from_utf8(stderr_bytes).expect("utf8");
    assert!(!stderr_out.contains("thinking"));
    assert!(!stderr_out.contains("response ready"));
    assert!(!stderr_out.contains("response chars="));
}

#[test]
fn spinner_is_disabled_on_non_tty_or_quiet_and_enabled_on_interactive_tty() {
    let mut non_tty_bytes = Vec::<u8>::new();
    {
        let mut renderer_non_tty = StderrUiRenderer::new(
            &mut non_tty_bytes,
            UiPolicy {
                quiet: false,
                verbosity: Verbosity::Normal,
            },
            false,
        );
        renderer_non_tty.emit(&UiEvent::LlmStarted);
        renderer_non_tty.flush();
    }
    assert!(
        non_tty_bytes.is_empty(),
        "non-tty should not render spinner"
    );

    let mut quiet_bytes = Vec::<u8>::new();
    {
        let mut renderer_quiet = StderrUiRenderer::new(
            &mut quiet_bytes,
            UiPolicy {
                quiet: true,
                verbosity: Verbosity::Quiet,
            },
            true,
        );
        renderer_quiet.emit(&UiEvent::LlmStarted);
        renderer_quiet.flush();
    }
    assert!(quiet_bytes.is_empty(), "quiet should not render spinner");

    let mut tty_bytes = Vec::<u8>::new();
    {
        let mut renderer_tty = StderrUiRenderer::new(
            &mut tty_bytes,
            UiPolicy {
                quiet: false,
                verbosity: Verbosity::Normal,
            },
            true,
        );
        renderer_tty.emit(&UiEvent::LlmStarted);
        renderer_tty.flush();
    }
    assert!(!tty_bytes.is_empty(), "tty should render spinner");
}

#[test]
fn spinner_pauses_for_persistent_lines_and_stops_on_completion() {
    let mut stderr_bytes = Vec::<u8>::new();
    {
        let mut renderer = StderrUiRenderer::new(
            &mut stderr_bytes,
            UiPolicy {
                quiet: false,
                verbosity: Verbosity::Normal,
            },
            true,
        );

        renderer.emit(&UiEvent::LlmStarted);

        renderer.emit(&UiEvent::Tick);

        renderer.emit(&UiEvent::ToolStarted {
            name: "t".to_string(),
            source: "closure".to_string(),
            arguments: "{}".to_string(),
        });

        renderer.emit(&UiEvent::Completed { tool_calls: 0 });
        renderer.flush();
    }

    let out = String::from_utf8(stderr_bytes).expect("utf8");
    assert!(out.contains("tool t → {}"), "tool spinner should render");
    assert!(out.contains("✓ completed"), "completed line should render");
}

#[test]
fn default_tool_lifecycle_is_single_completion_line_with_result_block() {
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Normal,
        },
        true,
    );

    renderer.emit(&UiEvent::LlmStarted);
    renderer.emit(&UiEvent::ToolStarted {
        name: "gh__list_prs".to_string(),
        source: "mcp".to_string(),
        arguments: "{}".to_string(),
    });
    renderer.emit(&UiEvent::ToolCompleted {
        name: "gh__list_prs".to_string(),
        source: "mcp".to_string(),
        arguments: "{}".to_string(),
        success: true,
        result: "[]".to_string(),
        display: None,
        error_kind: None,
        message: None,
    });
    renderer.flush();

    let stderr_out = String::from_utf8(stderr_bytes).expect("utf8");
    assert!(stderr_out.contains("✓ tool gh__list_prs → {}"));
    assert!(stderr_out.contains("\n[]"));
    assert!(!stderr_out.contains("→ tool gh__list_prs"));
}

#[test]
fn default_tool_lifecycle_prints_non_empty_payloads() {
    for payload in ["[]", "{}", "null", ""] {
        let mut stderr_bytes = Vec::<u8>::new();
        let mut renderer = StderrUiRenderer::new(
            &mut stderr_bytes,
            UiPolicy {
                quiet: false,
                verbosity: Verbosity::Normal,
            },
            true,
        );

        renderer.emit(&UiEvent::ToolStarted {
            name: "gh__list_prs".to_string(),
            source: "mcp".to_string(),
            arguments: "{}".to_string(),
        });
        renderer.emit(&UiEvent::ToolCompleted {
            name: "gh__list_prs".to_string(),
            source: "mcp".to_string(),
            arguments: "{}".to_string(),
            success: true,
            result: payload.to_string(),
            display: None,
            error_kind: None,
            message: None,
        });
        renderer.flush();

        let stderr_out = String::from_utf8(stderr_bytes).expect("utf8");
        assert!(stderr_out.contains("✓ tool gh__list_prs → {}"));
        if payload.is_empty() {
            assert!(!stderr_out.contains("\n[]"));
            assert!(!stderr_out.contains("\n{}"));
            assert!(!stderr_out.contains("\nnull"));
        } else {
            assert!(stderr_out.contains(&format!("\n{payload}")));
        }
    }
}

#[test]
fn spinner_tick_advances_frame_on_tty_only() {
    // Renderer that only starts the spinner (no tick)
    let mut start_bytes = Vec::<u8>::new();
    {
        let mut renderer = StderrUiRenderer::new(
            &mut start_bytes,
            UiPolicy {
                quiet: false,
                verbosity: Verbosity::Normal,
            },
            true,
        );
        renderer.emit(&UiEvent::LlmStarted);
        renderer.flush();
    }

    // Renderer that starts the spinner and ticks it
    let mut tick_bytes = Vec::<u8>::new();
    {
        let mut renderer = StderrUiRenderer::new(
            &mut tick_bytes,
            UiPolicy {
                quiet: false,
                verbosity: Verbosity::Normal,
            },
            true,
        );
        renderer.emit(&UiEvent::LlmStarted);
        renderer.emit(&UiEvent::Tick);
        renderer.flush();
    }

    assert_ne!(
        start_bytes, tick_bytes,
        "tick should advance the spinner frame"
    );

    let mut non_tty_bytes = Vec::<u8>::new();
    {
        let mut non_tty_renderer = StderrUiRenderer::new(
            &mut non_tty_bytes,
            UiPolicy {
                quiet: false,
                verbosity: Verbosity::Normal,
            },
            false,
        );
        non_tty_renderer.emit(&UiEvent::LlmStarted);
        non_tty_renderer.emit(&UiEvent::Tick);
        non_tty_renderer.flush();
    }
    assert!(
        non_tty_bytes.is_empty(),
        "non-tty should not render spinner"
    );
}

#[test]
fn turn_error_visible_at_default_verbosity_and_stops_spinner() {
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Normal,
        },
        true,
    );

    // Start spinner
    renderer.emit(&UiEvent::LlmStarted);
    renderer.emit(&UiEvent::Tick);

    // Emit TurnError
    renderer.emit(&UiEvent::TurnError {
        message: "Turn failed: Not authenticated. Run `agent auth login`.".to_string(),
    });
    renderer.flush();

    let stderr_out = String::from_utf8(stderr_bytes).expect("utf8");

    // Verify error is visible (not gated behind verbose flags)
    assert!(stderr_out.contains("Not authenticated"));
    assert!(stderr_out.contains("Error:"));
}
