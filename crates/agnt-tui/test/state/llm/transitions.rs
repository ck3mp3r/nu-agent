use super::*;

/// An LLM start from idle moves the phase to Busy and locks input; a start
/// while already busy is a phase no-op that keeps the status line.
#[test]
fn llm_start_transitions_phase_and_locks_input() -> Result<()> {
    // -- Setup & Fixtures
    let mut idle = AppState::default();

    // -- Exec
    reduce_llm(&mut idle, SystemEvent::InferenceStarted);

    // -- Check
    assert_eq!(idle.phase, UiPhase::Busy);
    assert!(idle.input_locked);

    // -- Setup & Fixtures: already busy.
    let mut busy = busy_state_with_clean_transcript();
    busy.status.message.set_message("Tool: prior");

    // -- Exec
    reduce_llm(&mut busy, SystemEvent::InferenceStarted);

    // -- Check
    assert_eq!(busy.phase, UiPhase::Busy);
    assert_eq!(busy.status.message.status_line(), "Tool: prior");
    assert!(busy.input_locked);

    // -- Setup & Fixtures: token usage recording.
    let mut state = AppState::default();

    // -- Exec
    reduce_llm(
        &mut state,
        SystemEvent::InferenceCompleted {
            usage: Usage {
                response_chars: 6,
                tool_calls: 0,
                input_tokens: 20,
                output_tokens: 10,
                total_tokens: Some(30),
                cost: Some(0.001),
                cost_complete: true,
            },
        },
    );

    // -- Check: latest values and the session total.
    assert_eq!(state.status.tokens.latest_input_tokens, Some(20));
    assert_eq!(state.status.tokens.latest_output_tokens, Some(10));
    assert_eq!(state.status.tokens.latest_total_tokens, Some(30));
    assert_eq!(state.status.tokens.session_total_tokens, 30);

    // -- Exec: a second completion accumulates the session total.
    reduce_llm(
        &mut state,
        SystemEvent::InferenceCompleted {
            usage: Usage {
                response_chars: 4,
                tool_calls: 0,
                input_tokens: 5,
                output_tokens: 7,
                total_tokens: Some(12),
                cost: Some(0.002),
                cost_complete: true,
            },
        },
    );

    // -- Check
    assert_eq!(state.status.tokens.latest_input_tokens, Some(5));
    assert_eq!(state.status.tokens.latest_output_tokens, Some(7));
    assert_eq!(state.status.tokens.latest_total_tokens, Some(12));
    assert_eq!(state.status.tokens.session_total_tokens, 42);

    // -- Setup & Fixtures: a busy state with a clean transcript.
    let mut busy = busy_state_with_clean_transcript();

    // -- Exec
    reduce_llm(
        &mut busy,
        SystemEvent::InferenceCompleted {
            usage: Usage {
                response_chars: 12,
                tool_calls: 0,
                input_tokens: 4,
                output_tokens: 8,
                total_tokens: Some(12),
                cost: None,
                cost_complete: false,
            },
        },
    );

    // -- Check: tokens recorded, status line untouched.
    assert_eq!(busy.status.tokens.latest_input_tokens, Some(4));
    assert_eq!(busy.status.tokens.latest_output_tokens, Some(8));
    assert_eq!(busy.status.tokens.latest_total_tokens, Some(12));
    assert_eq!(busy.status.tokens.session_total_tokens, 12);
    assert!(busy.status.message.status_line().is_empty());
    Ok(())
}
