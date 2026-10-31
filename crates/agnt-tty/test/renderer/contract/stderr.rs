//! Behavior tests for the stderr renderer.

use super::super::StderrUiRenderer;
use super::tool_call;
use agnt_core::protocol::events::{
    RequestId, SystemEvent, SystemKind, ToolCall, ToolOutcome, Usage,
};
use agnt_core::protocol::tool_args::CallLine;
use agnt_core::renderer::UiRenderer;
use agnt_core::session::tool_status::ToolStatus;
use agnt_core::transcript::ir::ToolName;

use crate::policy::{UiPolicy, Verbosity};

#[path = "stderr/permission_auto_resolved.rs"]
mod permission_auto_resolved;

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

    renderer.emit(&SystemEvent::InferenceStarted);
    renderer.emit(&SystemEvent::Tick);
    renderer.emit(&SystemEvent::InferenceCompleted {
        usage: Usage {
            response_chars: 5,
            tool_calls: 0,
            input_tokens: 3,
            output_tokens: 2,
            total_tokens: Some(5),
            cost: None,
            cost_complete: false,
        },
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

    renderer.emit(&SystemEvent::InferenceStarted);
    renderer.emit(&SystemEvent::Tick);
    renderer.emit(&SystemEvent::TurnCompleted {
        tool_calls: 0,
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: None,
        cost: None,
        cost_complete: true,
        duration_ms: 0,
    });
    renderer.emit(&SystemEvent::SystemMessage {
        kind: SystemKind::Warning,
        text: "essential warning".to_string(),
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

    renderer.emit(&SystemEvent::SystemMessage {
        kind: SystemKind::Warning,
        text: "important warning".to_string(),
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

    renderer.emit(&SystemEvent::InferenceStarted);
    renderer.emit(&SystemEvent::Tick);
    renderer.emit(&SystemEvent::InferenceCompleted {
        usage: Usage {
            response_chars: 42,
            tool_calls: 0,
            input_tokens: 30,
            output_tokens: 12,
            total_tokens: Some(42),
            cost: None,
            cost_complete: false,
        },
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
        renderer_non_tty.emit(&SystemEvent::InferenceStarted);
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
        renderer_quiet.emit(&SystemEvent::InferenceStarted);
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
        renderer_tty.emit(&SystemEvent::InferenceStarted);
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

        renderer.emit(&SystemEvent::InferenceStarted);

        renderer.emit(&SystemEvent::Tick);

        renderer.emit(&SystemEvent::ToolStarted {
            call: tool_call("t"),
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
    }

    let out = String::from_utf8(stderr_bytes).expect("utf8");
    assert!(out.contains("tool t → {}"), "tool spinner should render");
    assert!(out.contains("✓ completed"), "completed line should render");
}

#[test]
fn spinner_omits_arrow_when_call_line_summary_is_empty() {
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

        renderer.emit(&SystemEvent::InferenceStarted);

        renderer.emit(&SystemEvent::Tick);

        renderer.emit(&SystemEvent::ToolStarted {
            call: ToolCall::new(
                ToolName("nu".to_string()),
                "builtin",
                r#"{"command":"ls"}"#,
                CallLine {
                    summary: String::new(),
                },
            ),
        });

        renderer.flush();
    }

    let out = String::from_utf8(stderr_bytes).expect("utf8");
    assert!(
        out.contains("tool nu"),
        "tool spinner should render, got: {out}"
    );
    assert!(
        !out.contains('→'),
        "empty summary must not render a dangling arrow, got: {out}"
    );
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

    renderer.emit(&SystemEvent::InferenceStarted);
    renderer.emit(&SystemEvent::ToolStarted {
        call: ToolCall::new(
            ToolName("gh__list_prs".to_string()),
            "mcp",
            "{}",
            CallLine::from_json_summary("{}"),
        ),
    });
    renderer.emit(&SystemEvent::ToolCompleted {
        call: ToolCall::new(
            ToolName("gh__list_prs".to_string()),
            "mcp",
            "{}",
            CallLine::from_json_summary("{}"),
        ),
        outcome: ToolOutcome {
            success: true,
            status: ToolStatus::Success,
            result: "[]".to_string(),
            display: None,
            error_kind: None,
            message: None,
        },
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

        renderer.emit(&SystemEvent::ToolStarted {
            call: ToolCall::new(
                ToolName("gh__list_prs".to_string()),
                "mcp",
                "{}",
                CallLine::from_json_summary("{}"),
            ),
        });
        renderer.emit(&SystemEvent::ToolCompleted {
            call: ToolCall::new(
                ToolName("gh__list_prs".to_string()),
                "mcp",
                "{}",
                CallLine::from_json_summary("{}"),
            ),
            outcome: ToolOutcome {
                success: true,
                status: ToolStatus::Success,
                result: payload.to_string(),
                display: None,
                error_kind: None,
                message: None,
            },
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
        renderer.emit(&SystemEvent::InferenceStarted);
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
        renderer.emit(&SystemEvent::InferenceStarted);
        renderer.emit(&SystemEvent::Tick);
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
        non_tty_renderer.emit(&SystemEvent::InferenceStarted);
        non_tty_renderer.emit(&SystemEvent::Tick);
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
    renderer.emit(&SystemEvent::InferenceStarted);
    renderer.emit(&SystemEvent::Tick);

    // Emit TurnError
    renderer.emit(&SystemEvent::SystemMessage {
        kind: SystemKind::TurnError,
        text: "Turn failed: Not authenticated. Run `agent auth login`.".to_string(),
    });
    renderer.flush();

    let stderr_out = String::from_utf8(stderr_bytes).expect("utf8");

    // Verify error is visible (not gated behind verbose flags)
    assert!(stderr_out.contains("Not authenticated"));
    assert!(stderr_out.contains("Error:"));
}

#[test]
fn permission_request_cancelled_renders_line_and_quiet_suppresses() {
    // -- Setup & Fixtures
    let event = SystemEvent::PermissionRequestCancelled {
        request_id: RequestId::new("perm-0000000000000001"),
        matched_rule_identity: "global:*".to_string(),
    };

    // -- Exec
    let mut loud_bytes = Vec::<u8>::new();
    {
        let mut renderer = StderrUiRenderer::new(
            &mut loud_bytes,
            UiPolicy {
                quiet: false,
                verbosity: Verbosity::Normal,
            },
            false,
        );
        renderer.emit(&event);
        renderer.flush();
    }
    let mut quiet_bytes = Vec::<u8>::new();
    {
        let mut renderer = StderrUiRenderer::new(
            &mut quiet_bytes,
            UiPolicy {
                quiet: true,
                verbosity: Verbosity::Quiet,
            },
            false,
        );
        renderer.emit(&event);
        renderer.flush();
    }

    // -- Check
    let loud = String::from_utf8(loud_bytes).expect("utf8");
    assert!(
        loud.contains(
            "permission request cancelled: request_id=perm-0000000000000001 rule=global:*"
        ),
        "the cancelled line must name the request and the rule; got: {loud}"
    );
    let quiet = String::from_utf8(quiet_bytes).expect("utf8");
    assert!(
        quiet.is_empty(),
        "quiet mode must suppress the cancelled line; got: {quiet}"
    );
}
