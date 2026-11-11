//! Stop notices: the silent discard of provisional output and the streaming
//! reset it performs.

use super::*;

#[test]
fn stopped_with_empty_reason_prints_no_notice()
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
    narrate(
        &mut renderer,
        NarrationEvent::InferenceChunk {
            delta: "provisional".to_string(),
            aggregated: "provisional".to_string(),
        },
    );
    draw(
        &mut renderer,
        SessionRecord::Inference(InferenceRecord::Stopped {
            cause: agnt_kernel::StopCause::Other,
            reason: String::new(),
        }),
    );
    renderer.flush();

    let stderr_out = String::from_utf8(stderr_bytes)?;
    assert!(!stderr_out.contains("stopped:"));
    Ok(())
}

#[test]
fn stopped_with_empty_reason_resets_streaming_for_retry()
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
    narrate(
        &mut renderer,
        NarrationEvent::InferenceChunk {
            delta: "provisional".to_string(),
            aggregated: "provisional".to_string(),
        },
    );
    assert_eq!(
        renderer.streaming_printed_len,
        "provisional".len(),
        "the provisional chunk advanced the printed length"
    );

    draw(
        &mut renderer,
        SessionRecord::Inference(InferenceRecord::Stopped {
            cause: agnt_kernel::StopCause::Other,
            reason: String::new(),
        }),
    );

    // The stop resets the streaming state, so the retry is not sliced against
    // the discarded provisional length.
    assert_eq!(
        renderer.streaming_printed_len, 0,
        "the stop must reset the printed length"
    );

    // The retry's text is shorter than the discarded provisional text; without
    // the streaming reset it would be sliced away entirely.
    narrate(
        &mut renderer,
        NarrationEvent::InferenceChunk {
            delta: "retry".to_string(),
            aggregated: "retry".to_string(),
        },
    );
    assert_eq!(
        renderer.streaming_printed_len,
        "retry".len(),
        "the retry prints from its first byte"
    );
    draw(
        &mut renderer,
        SessionRecord::Turn(TurnRecord::Completed { duration_ms: 0 }),
    );
    renderer.flush();

    let stderr_out = String::from_utf8(stderr_bytes)?;
    assert!(stderr_out.contains("retry"));
    Ok(())
}
