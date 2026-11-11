//! Verbosity gating: which levels show warnings, and which show streaming.

use super::*;

#[test]
fn warning_requires_very_verbose() -> core::result::Result<(), Box<dyn std::error::Error>> {
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

    narrate(
        &mut renderer,
        NarrationEvent::SystemMessage {
            kind: NoticeKind::Warning,
            text: "test warning".to_string(),
        },
    );
    renderer.flush();

    let stderr_out = String::from_utf8(stderr_bytes)?;
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

    narrate(
        &mut renderer,
        NarrationEvent::SystemMessage {
            kind: NoticeKind::Warning,
            text: "test warning".to_string(),
        },
    );
    renderer.flush();

    let stderr_out = String::from_utf8(stderr_bytes)?;
    assert!(stderr_out.contains("test warning"));
    Ok(())
}

#[test]
fn very_verbose_shows_both_warnings_and_streaming()
-> core::result::Result<(), Box<dyn std::error::Error>> {
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::VeryVerbose,
        },
        false,
    );

    draw(
        &mut renderer,
        SessionRecord::Inference(InferenceRecord::Started),
    );
    narrate(
        &mut renderer,
        NarrationEvent::InferenceChunk {
            delta: "response text".to_string(),
            aggregated: "response text".to_string(),
        },
    );
    narrate(
        &mut renderer,
        NarrationEvent::SystemMessage {
            kind: NoticeKind::Warning,
            text: "test warning".to_string(),
        },
    );
    draw(
        &mut renderer,
        SessionRecord::Turn(TurnRecord::Completed { duration_ms: 0 }),
    );
    renderer.flush();

    let stderr_out = String::from_utf8(stderr_bytes)?;
    assert!(stderr_out.contains("response text"));
    assert!(stderr_out.contains("test warning"));
    Ok(())
}

#[test]
fn trace_mode_shows_streaming() -> core::result::Result<(), Box<dyn std::error::Error>> {
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Trace,
        },
        false,
    );

    draw(
        &mut renderer,
        SessionRecord::Inference(InferenceRecord::Started),
    );
    narrate(
        &mut renderer,
        NarrationEvent::InferenceChunk {
            delta: "trace output".to_string(),
            aggregated: "trace output".to_string(),
        },
    );
    draw(
        &mut renderer,
        SessionRecord::Turn(TurnRecord::Completed { duration_ms: 0 }),
    );
    renderer.flush();

    let stderr_out = String::from_utf8(stderr_bytes)?;
    assert!(stderr_out.contains("trace output"));
    Ok(())
}
