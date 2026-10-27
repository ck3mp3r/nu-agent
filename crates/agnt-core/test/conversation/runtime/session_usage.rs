//! The session-cumulative usage cells: what `session_ended` reports.
//!
//! The cells live on the runtime, are cloned into every turn's `ToolInfra`,
//! and reset only when the runtime switches sessions. Compaction shrinks the
//! context but never resets billing totals.

use rig::completion::Usage;
use rig::test_utils::{MockCompletionModel, MockStreamEvent};

use super::harness::*;
use super::*;
use crate::protocol::contracts::CoreRuntime;
use crate::protocol::session_management::{SessionPersistence, SessionState};

/// A model whose every turn reports `input`/`output` tokens and a total.
fn usage_model(turns: usize, input: u64, output: u64) -> MockCompletionModel {
    let usage = Usage::new()
        .input_tokens(input)
        .output_tokens(output)
        .total_tokens(input + output);
    let script: Vec<Vec<MockStreamEvent>> = (0..turns)
        .map(|_| {
            vec![
                MockStreamEvent::Text("reply".to_string()),
                MockStreamEvent::final_response(usage),
            ]
        })
        .collect();
    MockCompletionModel::from_stream_turns(script)
}

#[tokio::test]
async fn session_usage_accumulates_across_turns() -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = make_runtime(usage_model(2, 10, 5))?;

    // -- Exec: two turns on the same session.
    for _ in 0..2 {
        fixture
            .runtime
            .execute_turn(&fixture.bus, "hi".to_string(), None, Span::test_data())
            .await
            .map_err(|e| format!("execute_turn should succeed: {e}"))?;
    }
    let totals = fixture.runtime.session_usage();

    // -- Check: the session totals cover both turns.
    assert_eq!(
        totals.input_tokens, 20,
        "the session sum covers every turn's input tokens"
    );
    assert_eq!(
        totals.output_tokens, 10,
        "the session sum covers every turn's output tokens"
    );
    assert_eq!(
        totals.total_tokens,
        Some(30),
        "the session total sums each turn's total"
    );
    Ok(())
}

#[tokio::test]
async fn session_usage_resets_when_the_runtime_switches_sessions() -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = make_runtime(usage_model(1, 10, 5))?;
    fixture
        .runtime
        .execute_turn(&fixture.bus, "hi".to_string(), None, Span::test_data())
        .await
        .map_err(|e| format!("execute_turn should succeed: {e}"))?;
    assert_eq!(
        fixture.runtime.session_usage().input_tokens,
        10,
        "the fixture must start with a non-empty session sum"
    );

    // -- Exec: attach another session.
    fixture
        .runtime
        .attach_session("other-session")
        .await
        .map_err(|e| format!("attach_session should succeed: {e}"))?;

    // -- Check: the new session starts with no usage of its own.
    let totals = fixture.runtime.session_usage();
    assert_eq!(
        totals.input_tokens, 0,
        "a session switch must reset the cumulative cells"
    );
    assert_eq!(totals.output_tokens, 0);
    assert_eq!(totals.total_tokens, None);
    assert_eq!(totals.cost, None);
    Ok(())
}

#[tokio::test]
async fn session_usage_resets_when_a_session_is_loaded() -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = make_runtime(usage_model(1, 10, 5))?;
    fixture
        .runtime
        .execute_turn(&fixture.bus, "hi".to_string(), None, Span::test_data())
        .await
        .map_err(|e| format!("execute_turn should succeed: {e}"))?;
    let session_id = fixture
        .runtime
        .final_session_id
        .clone()
        .ok_or("fixture must carry a session id")?;
    assert_eq!(
        fixture.runtime.session_usage().input_tokens,
        10,
        "the fixture must start with a non-empty session sum"
    );

    // -- Exec: load the session the store holds after the turn.
    fixture
        .runtime
        .load_session(&session_id)
        .await
        .map_err(|e| format!("load_session should succeed: {e}"))?;

    // -- Check: the loaded session starts with no usage of its own.
    let totals = fixture.runtime.session_usage();
    assert_eq!(
        totals.input_tokens, 0,
        "loading a session must reset the cumulative cells"
    );
    assert_eq!(totals.output_tokens, 0);
    assert_eq!(totals.total_tokens, None);
    assert_eq!(totals.cost, None);
    Ok(())
}

#[tokio::test]
async fn session_usage_resets_on_a_new_session() -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = make_runtime(usage_model(1, 10, 5))?;
    fixture
        .runtime
        .execute_turn(&fixture.bus, "hi".to_string(), None, Span::test_data())
        .await
        .map_err(|e| format!("execute_turn should succeed: {e}"))?;

    // -- Exec
    fixture.runtime.new_session();

    // -- Check
    assert_eq!(
        fixture.runtime.session_usage().input_tokens,
        0,
        "a new session must start with no usage"
    );
    Ok(())
}

#[tokio::test]
async fn session_usage_survives_a_compaction() -> Result<()> {
    // -- Setup & Fixtures: a session with a conversation worth summarizing and
    // a turn's usage already accumulated.
    let mut fixture = make_runtime(usage_model(1, 10, 5))?;
    let session_id = fixture
        .runtime
        .final_session_id
        .clone()
        .ok_or("fixture must carry a session id")?;
    fixture
        .runtime
        .execute_turn(&fixture.bus, "hi".to_string(), None, Span::test_data())
        .await
        .map_err(|e| format!("execute_turn should succeed: {e}"))?;
    let before = fixture.runtime.session_usage();
    assert_eq!(before.input_tokens, 10, "the fixture must hold usage");
    assert!(
        fixture
            .runtime
            .last_total_tokens
            .lock()
            .map(|tokens| tokens.is_some())
            .unwrap_or(false),
        "the fixture must hold a real token count for the compaction reset to clear"
    );

    // -- Exec
    fixture
        .runtime
        .run_compaction("slash")
        .await
        .map_err(|e| format!("run_compaction should succeed: {e}"))?;

    // -- Check: compaction clears the context count but never the billing sum.
    assert_eq!(
        fixture.runtime.session_usage(),
        before,
        "compaction must not touch the session-cumulative cells"
    );
    assert_eq!(
        *fixture
            .runtime
            .last_total_tokens
            .lock()
            .map_err(|_| "token mutex poisoned")?,
        None,
        "compaction must still reset the context token count"
    );
    let _ = session_id;
    Ok(())
}
