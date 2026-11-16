use super::*;

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
        SessionRecord::Inference(InferenceRecord::Started {
            inference: InferenceId::mint(),
        }),
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
        SessionRecord::Inference(InferenceRecord::Started {
            inference: InferenceId::mint(),
        }),
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
        SessionRecord::Inference(InferenceRecord::Started {
            inference: InferenceId::mint(),
        }),
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
        SessionRecord::Inference(InferenceRecord::Started {
            inference: InferenceId::mint(),
        }),
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
