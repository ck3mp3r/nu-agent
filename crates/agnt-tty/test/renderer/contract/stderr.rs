//! Behavior tests for the stderr renderer.

use super::super::StderrUiRenderer;
use super::{draw, narrate, tool_call};
use crate::renderer::UiRenderer;
use agnt_kernel::bus::NarrationEvent;
use agnt_kernel::{
    InferenceRecord, NoticeKind, PermissionRecord, RequestId, SessionRecord, ToolCall, ToolOutcome,
    ToolRecord, ToolStatus, TurnRecord, Usage,
};

use crate::policy::{UiPolicy, Verbosity};

#[path = "stderr/permission_auto_resolved.rs"]
mod permission_auto_resolved;

#[test]
fn stderr_renderer_writes_only_to_stderr_sink()
-> core::result::Result<(), Box<dyn std::error::Error>> {
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Normal,
        },
        false,
    );

    draw(
        &mut renderer,
        SessionRecord::Inference(InferenceRecord::Started),
    );
    renderer.tick();
    draw(
        &mut renderer,
        SessionRecord::Inference(InferenceRecord::Completed {
            usage: Usage {
                input_tokens: 3,
                output_tokens: 2,
                total_tokens: Some(5),
                cost: None,
                cost_complete: false,
            },
        }),
    );
    renderer.flush();

    let stderr_out = String::from_utf8(stderr_bytes)?;
    assert!(stderr_out.trim().is_empty());
    Ok(())
}

#[test]
fn quiet_mode_suppresses_non_essential_progress_and_warnings()
-> core::result::Result<(), Box<dyn std::error::Error>> {
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: true,
            verbosity: Verbosity::Quiet,
        },
        true,
    );

    draw(
        &mut renderer,
        SessionRecord::Inference(InferenceRecord::Started),
    );
    renderer.tick();
    draw(
        &mut renderer,
        SessionRecord::Turn(TurnRecord::Completed { duration_ms: 0 }),
    );
    narrate(
        &mut renderer,
        NarrationEvent::SystemMessage {
            kind: NoticeKind::Warning,
            text: "essential warning".to_string(),
        },
    );
    renderer.flush();

    let stderr_out = String::from_utf8(stderr_bytes)?;
    assert!(!stderr_out.contains("thinking"));
    assert!(!stderr_out.contains("completed"));
    assert!(!stderr_out.contains("essential warning"));
    Ok(())
}

#[test]
fn verbose_mode_shows_warnings() -> core::result::Result<(), Box<dyn std::error::Error>> {
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::VeryVerbose,
        },
        true,
    );

    narrate(
        &mut renderer,
        NarrationEvent::SystemMessage {
            kind: NoticeKind::Warning,
            text: "important warning".to_string(),
        },
    );
    renderer.flush();

    let stderr_out = String::from_utf8(stderr_bytes)?;
    assert!(stderr_out.contains("important warning"));
    Ok(())
}

#[test]
fn default_busy_flow_uses_spinner_without_redundant_persistent_busy_lines()
-> core::result::Result<(), Box<dyn std::error::Error>> {
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Normal,
        },
        true,
    );

    draw(
        &mut renderer,
        SessionRecord::Inference(InferenceRecord::Started),
    );
    renderer.tick();
    draw(
        &mut renderer,
        SessionRecord::Inference(InferenceRecord::Completed {
            usage: Usage {
                input_tokens: 30,
                output_tokens: 12,
                total_tokens: Some(42),
                cost: None,
                cost_complete: false,
            },
        }),
    );
    renderer.flush();

    let stderr_out = String::from_utf8(stderr_bytes)?;
    assert!(!stderr_out.contains("thinking"));
    assert!(!stderr_out.contains("response ready"));
    assert!(!stderr_out.contains("response chars="));
    Ok(())
}

#[test]
fn spinner_is_disabled_on_non_tty_or_quiet_and_enabled_on_interactive_tty()
-> core::result::Result<(), Box<dyn std::error::Error>> {
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
        draw(
            &mut renderer_non_tty,
            SessionRecord::Inference(InferenceRecord::Started),
        );
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
        draw(
            &mut renderer_quiet,
            SessionRecord::Inference(InferenceRecord::Started),
        );
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
        draw(
            &mut renderer_tty,
            SessionRecord::Inference(InferenceRecord::Started),
        );
        renderer_tty.flush();
    }
    assert!(!tty_bytes.is_empty(), "tty should render spinner");
    Ok(())
}

#[test]
fn spinner_pauses_for_persistent_lines_and_stops_on_completion()
-> core::result::Result<(), Box<dyn std::error::Error>> {
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

        draw(
            &mut renderer,
            SessionRecord::Inference(InferenceRecord::Started),
        );

        renderer.tick();

        draw(
            &mut renderer,
            SessionRecord::Tool(ToolRecord::Started {
                call: tool_call("t"),
            }),
        );

        draw(
            &mut renderer,
            SessionRecord::Turn(TurnRecord::Completed { duration_ms: 0 }),
        );
        renderer.flush();
    }

    let out = String::from_utf8(stderr_bytes)?;
    assert!(out.contains("tool t → {}"), "tool spinner should render");
    assert!(out.contains("✓ completed"), "completed line should render");
    Ok(())
}

#[test]
fn spinner_omits_arrow_when_call_line_summary_is_empty()
-> core::result::Result<(), Box<dyn std::error::Error>> {
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

        draw(
            &mut renderer,
            SessionRecord::Inference(InferenceRecord::Started),
        );

        renderer.tick();

        draw(
            &mut renderer,
            SessionRecord::Tool(ToolRecord::Started {
                call: ToolCall::new("nu", agnt_runtime::tools::handler::ToolSource::Builtin, ""),
            }),
        );

        renderer.flush();
    }

    let out = String::from_utf8(stderr_bytes)?;
    assert!(
        out.contains("tool nu"),
        "tool spinner should render, got: {out}"
    );
    assert!(
        !out.contains('→'),
        "empty summary must not render a dangling arrow, got: {out}"
    );
    Ok(())
}

#[test]
fn default_tool_lifecycle_is_single_completion_line_with_result_block()
-> core::result::Result<(), Box<dyn std::error::Error>> {
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Normal,
        },
        true,
    );

    draw(
        &mut renderer,
        SessionRecord::Inference(InferenceRecord::Started),
    );
    draw(
        &mut renderer,
        SessionRecord::Tool(ToolRecord::Started {
            call: ToolCall::new(
                "gh__list_prs",
                agnt_runtime::tools::handler::ToolSource::Mcp,
                "{}",
            ),
        }),
    );
    draw(
        &mut renderer,
        SessionRecord::Tool(ToolRecord::Completed {
            call: ToolCall::new(
                "gh__list_prs",
                agnt_runtime::tools::handler::ToolSource::Mcp,
                "{}",
            ),
            outcome: ToolOutcome {
                status: ToolStatus::Success,
                payload: None,
                text: "[]".to_string(),
                message: None,
                kind: None,
            },
        }),
    );
    renderer.flush();

    let stderr_out = String::from_utf8(stderr_bytes)?;
    assert!(stderr_out.contains("✓ tool gh__list_prs → {}"));
    assert!(stderr_out.contains("\n[]"));
    assert!(!stderr_out.contains("→ tool gh__list_prs"));
    Ok(())
}

#[test]
fn default_tool_lifecycle_prints_non_empty_payloads()
-> core::result::Result<(), Box<dyn std::error::Error>> {
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

        draw(
            &mut renderer,
            SessionRecord::Tool(ToolRecord::Started {
                call: ToolCall::new(
                    "gh__list_prs",
                    agnt_runtime::tools::handler::ToolSource::Mcp,
                    "{}",
                ),
            }),
        );
        draw(
            &mut renderer,
            SessionRecord::Tool(ToolRecord::Completed {
                call: ToolCall::new(
                    "gh__list_prs",
                    agnt_runtime::tools::handler::ToolSource::Mcp,
                    "{}",
                ),
                outcome: ToolOutcome {
                    status: ToolStatus::Success,
                    payload: None,
                    text: payload.to_string(),
                    message: None,
                    kind: None,
                },
            }),
        );
        renderer.flush();

        let stderr_out = String::from_utf8(stderr_bytes)?;
        assert!(stderr_out.contains("✓ tool gh__list_prs → {}"));
        if payload.is_empty() {
            assert!(!stderr_out.contains("\n[]"));
            assert!(!stderr_out.contains("\n{}"));
            assert!(!stderr_out.contains("\nnull"));
        } else {
            assert!(stderr_out.contains(&format!("\n{payload}")));
        }
    }
    Ok(())
}

#[test]
fn spinner_tick_advances_frame_on_tty_only() -> core::result::Result<(), Box<dyn std::error::Error>>
{
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
        draw(
            &mut renderer,
            SessionRecord::Inference(InferenceRecord::Started),
        );
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
        draw(
            &mut renderer,
            SessionRecord::Inference(InferenceRecord::Started),
        );
        renderer.tick();
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
        draw(
            &mut non_tty_renderer,
            SessionRecord::Inference(InferenceRecord::Started),
        );
        non_tty_renderer.tick();
        non_tty_renderer.flush();
    }
    assert!(
        non_tty_bytes.is_empty(),
        "non-tty should not render spinner"
    );
    Ok(())
}

#[test]
fn turn_error_visible_at_default_verbosity_and_stops_spinner()
-> core::result::Result<(), Box<dyn std::error::Error>> {
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
    draw(
        &mut renderer,
        SessionRecord::Inference(InferenceRecord::Started),
    );
    renderer.tick();

    // Emit TurnError
    narrate(
        &mut renderer,
        NarrationEvent::SystemMessage {
            kind: NoticeKind::TurnError,
            text: "Turn failed: Not authenticated. Run `agent auth login`.".to_string(),
        },
    );
    renderer.flush();

    let stderr_out = String::from_utf8(stderr_bytes)?;

    // Verify error is visible (not gated behind verbose flags)
    assert!(stderr_out.contains("Not authenticated"));
    assert!(stderr_out.contains("Error:"));
    Ok(())
}

#[test]
fn permission_request_cancelled_renders_line_and_quiet_suppresses()
-> core::result::Result<(), Box<dyn std::error::Error>> {
    // -- Setup & Fixtures
    let record = SessionRecord::Permission(PermissionRecord::Cancelled {
        request_id: RequestId::from_minted("perm-0000000000000001"),
        rule_identity: "global:*".to_string(),
    });

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
        draw(&mut renderer, record.clone());
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
        draw(&mut renderer, record.clone());
        renderer.flush();
    }

    // -- Check
    let loud = String::from_utf8(loud_bytes)?;
    assert!(
        loud.contains(
            "permission request cancelled: request_id=perm-0000000000000001 rule=global:*"
        ),
        "the cancelled line must name the request and the rule; got: {loud}"
    );
    let quiet = String::from_utf8(quiet_bytes)?;
    assert!(
        quiet.is_empty(),
        "quiet mode must suppress the cancelled line; got: {quiet}"
    );
    Ok(())
}
