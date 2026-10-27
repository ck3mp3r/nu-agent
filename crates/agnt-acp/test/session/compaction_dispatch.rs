//! The post-turn compaction dispatcher: a turn that trips the chain's
//! threshold runs one compaction, and the client sees it.
//!
//! The trigger is the chain's own auto rule (`decide_compaction`), which fires
//! in every mode. ACP adds no threshold logic: it consumes the request the
//! chain publishes and runs the compaction post-turn.

use super::*;

/// Every compaction state the sink recorded, in order.
fn recorded_compaction_states(sink: &RecordingSink) -> Vec<String> {
    sink.taken()
        .iter()
        .filter_map(|update| {
            let SessionUpdate::SessionInfoUpdate(info) = update else {
                return None;
            };
            info.meta
                .as_ref()
                .and_then(|meta| meta.get("nu-agent"))
                .and_then(|compaction| compaction.get("state"))
                .and_then(|state| state.as_str())
                .map(|state| state.to_string())
        })
        .collect()
}

/// A prompt long enough that a few turns outgrow the policy's keep window, so
/// the compactor has something to summarize.
fn long_prompt() -> String {
    "lorem ipsum ".repeat(400)
}

#[tokio::test]
async fn test_session_manager_prompt_runs_a_requested_compaction_post_turn() -> Result<()> {
    // -- Setup & Fixtures: a threshold any non-empty conversation trips, and a
    // model scripted for the turn plus the summarizer's call.
    let temp = tempfile::tempdir()?;
    let manager = manager_with_builder(
        temp.path(),
        MockRuntimeBuilder::default()
            .with_threshold_tokens(1)
            .with_stream_turns(32),
    )
    .await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;
    let sink = RecordingSink::default();

    // -- Exec: the first turn populates the store; later turns trip the
    // threshold against it, and the conversation outgrows the keep window so
    // the compactor actually summarizes.
    let mut stop_reason = StopReason::EndTurn;
    for _ in 0..4 {
        stop_reason = manager.prompt(&session_id, long_prompt(), &sink).await?;
    }

    // -- Check: the client saw a compaction start and finish, and the stop
    // reason is the turn's own.
    let states = recorded_compaction_states(&sink);
    let started = states
        .iter()
        .position(|state| state == "started")
        .ok_or(format!(
            "a tripped threshold must report compaction_started; got: {states:?}"
        ))?;
    assert!(
        states[started..].iter().any(|state| state == "completed"),
        "the started compaction must report completion; got: {states:?}"
    );
    assert_eq!(
        stop_reason,
        StopReason::EndTurn,
        "the post-turn compaction must not change the turn's stop reason"
    );
    Ok(())
}

#[tokio::test]
async fn test_session_manager_prompt_under_threshold_emits_no_compaction() -> Result<()> {
    // -- Setup & Fixtures: no threshold, so the chain never requests one.
    let temp = tempfile::tempdir()?;
    let manager = manager(temp.path()).await?;
    let cwd = session_cwd(temp.path(), "session-cwd")?;
    let session_id = manager.new_session(cwd, Vec::new()).await?;
    let sink = RecordingSink::default();

    // -- Exec
    let _ = manager
        .prompt(&session_id, "hello".to_string(), &sink)
        .await?;

    // -- Check
    assert!(
        recorded_compaction_states(&sink).is_empty(),
        "a turn under the threshold must emit no compaction update; got: {:?}",
        sink.taken()
    );
    Ok(())
}
