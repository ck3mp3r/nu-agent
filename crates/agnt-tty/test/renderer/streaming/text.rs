//! Streaming text: what the renderer prints, how the printed length grows with
//! each chunk, and how it resets between sequences.
//!
//! Every streaming fixture carries the two fields a real producer sends —
//! `delta` is the newly received text, `aggregated` is the accumulated text —
//! and the assertions read the printer's state BETWEEN chunks, so a renderer
//! that only survives a single aggregated payload fails here.

use super::*;

fn renderer_with_policy(
    sink: &SharedSink,
    quiet: bool,
    verbosity: Verbosity,
    stderr_is_tty: bool,
) -> StderrUiRenderer<SharedSink> {
    StderrUiRenderer::new(sink.clone(), UiPolicy { quiet, verbosity }, stderr_is_tty)
}

fn turn_completed_event() -> SystemEvent {
    SystemEvent::TurnCompleted {
        tool_calls: 0,
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: None,
        cost: None,
        cost_complete: true,
        duration_ms: 0,
    }
}

fn inference_completed_event() -> SystemEvent {
    SystemEvent::InferenceCompleted {
        usage: Usage {
            response_chars: 0,
            tool_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: None,
            cost: None,
            cost_complete: true,
        },
    }
}

#[test]
fn normal_mode_streams_text() {
    // -- Setup & Fixtures: the non-streaming producer's shape — one chunk, both
    // fields equal to the full text.
    let sink = SharedSink::new();
    let mut renderer = renderer_with_policy(&sink, false, Verbosity::Normal, false);

    // -- Exec
    renderer.emit(&SystemEvent::InferenceStarted);
    renderer.emit(&SystemEvent::InferenceChunk {
        delta: "hello world".to_string(),
        aggregated: "hello world".to_string(),
    });
    renderer.emit(&turn_completed_event());
    renderer.flush();

    // -- Check: the full text is rendered exactly once, not re-printed.
    let out = sink.snapshot();
    assert!(out.contains("hello world"), "got {out:?}");
    assert_eq!(out.matches("hello world").count(), 1, "got {out:?}");
}

#[test]
fn normal_mode_prints_each_chunk_new_suffix() -> Result<()> {
    // -- Setup & Fixtures: per-chunk delta/aggregated pairs, as chain.rs emits
    // them: `delta` is the newly received text, `aggregated` the accumulated
    // text.
    let sink = SharedSink::new();
    let mut renderer = renderer_with_policy(&sink, false, Verbosity::Normal, false);
    let chunks = [
        ("hello", "hello"),
        (" ", "hello "),
        ("world", "hello world"),
    ];

    // -- Exec
    renderer.emit(&SystemEvent::InferenceStarted);

    // -- Check: nothing printed before the first chunk.
    assert_eq!(renderer.streaming_printed_len, 0, "nothing printed yet");

    // -- Exec & Check: every chunk advances the printed length by its own
    // suffix and the sink holds exactly the accumulated text so far.
    for (index, (delta, aggregated)) in chunks.iter().enumerate() {
        renderer.emit(&SystemEvent::InferenceChunk {
            delta: delta.to_string(),
            aggregated: aggregated.to_string(),
        });
        assert_eq!(
            renderer.streaming_printed_len,
            aggregated.len(),
            "printed length after chunk {index}"
        );
        assert_eq!(sink.snapshot(), *aggregated, "output after chunk {index}");
    }

    // -- Exec: the turn closes.
    renderer.emit(&turn_completed_event());
    renderer.flush();

    // -- Check: the accumulated text is emitted once and closed by the
    // completion newline; the turn footer follows it.
    let out = sink.snapshot();
    assert!(out.starts_with("hello world\n"), "got {out:?}");
    assert_eq!(out.matches("hello world").count(), 1, "got {out:?}");
    Ok(())
}

#[test]
fn quiet_mode_suppresses_streaming() {
    // -- Setup & Fixtures
    let sink = SharedSink::new();
    let mut renderer = renderer_with_policy(&sink, true, Verbosity::Quiet, false);

    // -- Exec
    renderer.emit(&SystemEvent::InferenceStarted);
    renderer.emit(&SystemEvent::InferenceChunk {
        delta: "hello world".to_string(),
        aggregated: "hello world".to_string(),
    });
    renderer.emit(&turn_completed_event());
    renderer.flush();

    // -- Check
    assert!(!sink.snapshot().contains("hello world"));
    assert_eq!(
        renderer.streaming_printed_len, 0,
        "quiet mode prints nothing"
    );
}

#[test]
fn streaming_completed_resets_state() -> Result<()> {
    // -- Setup & Fixtures
    let sink = SharedSink::new();
    let mut renderer = renderer_with_policy(&sink, false, Verbosity::Normal, false);

    // -- Exec: first streaming sequence.
    renderer.emit(&SystemEvent::InferenceStarted);
    renderer.emit(&SystemEvent::InferenceChunk {
        delta: "first".to_string(),
        aggregated: "first".to_string(),
    });

    // -- Check: the first sequence printed its text and advanced its length.
    assert_eq!(sink.snapshot(), "first");
    assert_eq!(renderer.streaming_printed_len, "first".len());

    // -- Exec: close the sequence, then start a second one.
    renderer.emit(&turn_completed_event());
    renderer.emit(&SystemEvent::InferenceStarted);
    renderer.emit(&SystemEvent::InferenceChunk {
        delta: "second".to_string(),
        aggregated: "second".to_string(),
    });

    // -- Check: the reset makes the second sequence print from its own first
    // byte instead of being sliced against the first sequence's length.
    assert_eq!(renderer.streaming_printed_len, "second".len());
    assert!(
        sink.snapshot().ends_with("second"),
        "the second sequence must print its own text, got {:?}",
        sink.snapshot()
    );

    // -- Exec
    renderer.emit(&turn_completed_event());
    renderer.flush();

    // -- Check: both sequences landed on their own lines.
    let out = sink.snapshot();
    assert!(out.contains("first"));
    assert!(out.contains("second"));
    assert!(
        out.lines().count() >= 2,
        "expected at least 2 lines: {out:?}"
    );
    Ok(())
}

#[test]
fn normal_mode_incremental_streaming() -> Result<()> {
    // -- Setup & Fixtures: three chunks whose deltas are the new suffixes and
    // whose aggregated values accumulate them — every intermediate state is
    // distinct.
    let sink = SharedSink::new();
    let mut renderer = renderer_with_policy(&sink, false, Verbosity::Normal, false);
    let chunks = [("a", "a"), ("b", "ab"), ("c", "abc")];

    // -- Exec
    renderer.emit(&SystemEvent::InferenceStarted);

    // -- Check: nothing printed before the first chunk.
    assert_eq!(renderer.streaming_printed_len, 0, "nothing printed yet");

    // -- Exec & Check: every chunk advances the printed length to its own
    // accumulated text and the sink holds exactly that text so far.
    for (index, (delta, aggregated)) in chunks.iter().enumerate() {
        renderer.emit(&SystemEvent::InferenceChunk {
            delta: delta.to_string(),
            aggregated: aggregated.to_string(),
        });
        assert_eq!(
            renderer.streaming_printed_len,
            aggregated.len(),
            "printed length after chunk {index}"
        );
        assert_eq!(sink.snapshot(), *aggregated, "output after chunk {index}");
    }

    // -- Exec: the inference closes.
    renderer.emit(&inference_completed_event());
    renderer.flush();

    // -- Check: the accumulated text printed once — no repetition, no lost
    // suffix, no garbage.
    let out = sink.snapshot();
    assert_eq!(out, "abc\n");
    assert_eq!(out.matches("abc").count(), 1);
    assert!(
        !out.contains("aababc"),
        "no re-print of earlier prefixes: {out:?}"
    );
    Ok(())
}

#[test]
fn spinner_stops_when_streaming_starts() -> Result<()> {
    // -- Setup & Fixtures
    let sink = SharedSink::new();
    let mut renderer = renderer_with_policy(&sink, false, Verbosity::Normal, true);

    // -- Exec: the spinner starts with the inference call.
    renderer.emit(&SystemEvent::InferenceStarted);

    // -- Check: the spinner owns the line before the first chunk.
    assert_eq!(renderer.streaming_printed_len, 0);

    // -- Exec: the first chunk stops the spinner and prints the text.
    renderer.emit(&SystemEvent::InferenceChunk {
        delta: "streaming text".to_string(),
        aggregated: "streaming text".to_string(),
    });

    // -- Check
    assert_eq!(renderer.streaming_printed_len, "streaming text".len());
    assert!(sink.snapshot().contains("streaming text"));

    // -- Exec: the second chunk appends its suffix only.
    renderer.emit(&SystemEvent::InferenceChunk {
        delta: " continues".to_string(),
        aggregated: "streaming text continues".to_string(),
    });
    renderer.emit(&turn_completed_event());
    renderer.flush();

    // -- Check
    let out = sink.snapshot();
    assert!(out.contains("streaming text continues"));
    assert_eq!(out.matches("streaming text continues").count(), 1);
    Ok(())
}

#[test]
fn streaming_adds_newline_on_completion() {
    // -- Setup & Fixtures
    let sink = SharedSink::new();
    let mut renderer = renderer_with_policy(&sink, false, Verbosity::Normal, false);

    // -- Exec
    renderer.emit(&SystemEvent::InferenceStarted);
    renderer.emit(&SystemEvent::InferenceChunk {
        delta: "no newline yet".to_string(),
        aggregated: "no newline yet".to_string(),
    });
    renderer.emit(&turn_completed_event());
    renderer.flush();

    // -- Check: the text is un-terminated while streaming, terminated after.
    assert!(sink.snapshot().ends_with('\n'));
}
