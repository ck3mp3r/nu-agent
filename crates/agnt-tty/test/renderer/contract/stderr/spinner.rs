use super::*;

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
            SessionRecord::Inference(InferenceRecord::Started {
                inference: InferenceId::mint(),
            }),
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
            SessionRecord::Inference(InferenceRecord::Started {
                inference: InferenceId::mint(),
            }),
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
            SessionRecord::Inference(InferenceRecord::Started {
                inference: InferenceId::mint(),
            }),
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
            SessionRecord::Inference(InferenceRecord::Started {
                inference: InferenceId::mint(),
            }),
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
            SessionRecord::Inference(InferenceRecord::Started {
                inference: InferenceId::mint(),
            }),
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
            SessionRecord::Inference(InferenceRecord::Started {
                inference: InferenceId::mint(),
            }),
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
            SessionRecord::Inference(InferenceRecord::Started {
                inference: InferenceId::mint(),
            }),
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
            SessionRecord::Inference(InferenceRecord::Started {
                inference: InferenceId::mint(),
            }),
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
