//! The ladder's middle rung: the near-window clearing — the checkpoint rule, the
//! notes a clearing asks for, and the plan the executor persists.

use super::super::*;
use super::*;

use crate::hook::chain::in_place::InPlace;
use crate::types::{
    AssistantContent, AssistantMessage, CallId, ToolCall, ToolFunction, ToolResult,
    ToolResultContent, UserContent,
};
use rig_memory::{ClearToolOutputs, NearWindow, TrackArgument};

// region:    --- Test Support

/// A policy whose clearing takes every output above one token: the tests pin the
/// ladder, so the sizes that free little are not what they exercise.
fn clearing_policy() -> rig_memory::CompactionPolicy {
    rig_memory::CompactionPolicy {
        clearing: ClearToolOutputs {
            keep_tokens: 0,
            keep_under: 1,
        },
        ..rig_memory::CompactionPolicy::default()
    }
}

/// A `read` call of `path`, answering `call`.
fn read_call(call: &str, path: &str) -> Result<ToolCall> {
    Ok(ToolCall::new(
        CallId::from_wire(call),
        ToolFunction::new(
            ToolName::new("read").map_err(|_| "read is a non-empty tool name")?,
            serde_json::json!({ "path": path }),
        ),
    ))
}

/// The assistant turn carrying `call`.
fn call_turn(call: ToolCall) -> Message {
    Message::Assistant(AssistantMessage::new(vec![AssistantContent::ToolCall(
        call,
    )]))
}

/// The user turn carrying the result of `call`: `size` characters of output.
fn result_turn(call: &str, size: usize) -> Result<Message> {
    let result = ToolResult {
        call: CallId::from_wire(call),
        name: ToolName::new("read").map_err(|_| "read is a non-empty tool name")?,
        content: vec![ToolResultContent::text("o".repeat(size))],
        is_error: false,
    };
    Ok(Message::User {
        content: vec![UserContent::ToolResult(result)],
    })
}

/// The user text parts of `message`, concatenated.
fn user_text(message: &Message) -> String {
    let Message::User { content } = message else {
        return String::new();
    };
    content
        .iter()
        .filter_map(|item| match item {
            UserContent::Text(text) => Some(text.text.clone()),
            _ => None,
        })
        .collect()
}

/// The tool-result texts of `message`, concatenated.
fn result_text(message: &Message) -> String {
    let Message::User { content } = message else {
        return String::new();
    };
    content
        .iter()
        .filter_map(|item| match item {
            UserContent::ToolResult(result) => Some(
                result
                    .content
                    .iter()
                    .filter_map(|content| content.as_text().map(str::to_string))
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect()
}

/// The history a clearing patch sends, `None` when the decision did not patch.
fn patched_history(
    action: &rig::agent::CompletionCallAction,
) -> Option<Vec<std::sync::Arc<Message>>> {
    match action {
        rig::agent::CompletionCallAction::Patch(patch) => patch.history.clone(),
        _ => None,
    }
}

// endregion: --- Test Support

/// WHEN the in-place request is impossible and clearing frees enough of the room,
/// THEN THE decision SHALL patch this call with the cleared history, stash the
/// plan for the executor, and — because the dropped output cannot be had again —
/// run exactly one notes call before the save.
#[tokio::test]
async fn decide_compaction_clears_and_asks_for_notes() -> Result<()> {
    // -- Setup & Fixtures: one tool output of ~50k tokens against a 60k limit
    // whose reserve (20 percent of the limit) leaves 48_000 usable, and a cut
    // the clearing leaves nothing to
    // summarize from — so the summary cannot be asked in place.
    let fx = decide_fixture_with_policy(clearing_policy())?;
    let spec = spec_with_input_limit(60_000)?;
    seed_rig(&fx, &[result_turn("tc-1", 200_000)?]).await?;
    let mut control_rx = fx.bus.control().subscribe();

    // -- Exec
    let action = decide_compaction(
        &Message::user("hi"),
        "conv-1",
        InPlace {
            spec: Some(&spec),
            pending: &fx.pending,
        },
        &CompactionPath::new(
            fx.memory.as_ref(),
            &fx.compaction,
            &fx.meter,
            &Default::default(),
            &fx.model,
        ),
        fx.bus.stream(),
        fx.bus.lifecycle(),
    )
    .await;

    // -- Check: the patch carries the placeholder and the notes, nothing reached
    // the control channel, and the plan waits for the executor.
    let sent = patched_history(&action)
        .ok_or_else(|| format!("the clearing must patch this call, got {action:?}"))?;
    let placeholder = ClearToolOutputs::PLACEHOLDER;
    assert!(
        sent.iter()
            .any(|message| result_text(message) == placeholder),
        "the patch must send the placeholder instead of the cleared output"
    );
    assert!(
        sent.iter()
            .any(|message| user_text(message).contains("<cleared-output-notes>")),
        "the patch must carry the notes written on the cleared output"
    );
    assert!(
        fx.pending.take_clear().is_some(),
        "the decision must stash the clearing the executor persists"
    );
    assert!(
        matches!(
            control_rx.try_recv(),
            Err(crate::bus::TryReceiveError::Empty)
        ),
        "auto compaction SHALL NOT schedule a worker compaction"
    );
    assert_eq!(
        fx.turn_model.requests().len(),
        1,
        "a clearing whose output cannot be had again asks for notes once"
    );
    Ok(())
}

/// WHEN every output a clearing drops can be had again (its call is a `tracked`
/// tool and its path still stats), THEN THE decision SHALL clear silently: no
/// notes call and no notes in the patch.
#[tokio::test]
async fn decide_compaction_clears_silently_when_the_output_can_be_had_again() -> Result<()> {
    // -- Setup & Fixtures: a `read` call whose path exists, and the policy that
    // tracks it.
    let temp_dir = TempDir::new().map_err(|_| "should create temp dir")?;
    let path = temp_dir.path().join("notes.txt");
    std::fs::write(&path, b"the file the call read").map_err(|_| "should write the file")?;
    let tracked = path.to_str().ok_or("the temp path is UTF-8")?;
    let mut policy = clearing_policy();
    policy.tracked = vec![TrackArgument::new("read", "path", "read-files")];
    let fx = decide_fixture_with_policy(policy)?;
    let spec = spec_with_input_limit(60_000)?;
    seed_rig(
        &fx,
        &[
            call_turn(read_call("tc-1", tracked)?),
            result_turn("tc-1", 200_000)?,
        ],
    )
    .await?;

    // -- Exec
    let action = decide_compaction(
        &Message::user("hi"),
        "conv-1",
        InPlace {
            spec: Some(&spec),
            pending: &fx.pending,
        },
        &CompactionPath::new(
            fx.memory.as_ref(),
            &fx.compaction,
            &fx.meter,
            &Default::default(),
            &fx.model,
        ),
        fx.bus.stream(),
        fx.bus.lifecycle(),
    )
    .await;

    // -- Check
    let sent = patched_history(&action)
        .ok_or_else(|| format!("the clearing must patch this call, got {action:?}"))?;
    assert!(
        sent.iter()
            .any(|message| result_text(message) == ClearToolOutputs::PLACEHOLDER),
        "the patch must send the placeholder instead of the cleared output"
    );
    assert!(
        sent.iter()
            .all(|message| !user_text(message).contains("<cleared-output-notes>")),
        "an output that can be had again asks for no notes"
    );
    assert!(
        fx.pending.take_clear().is_some(),
        "the clearing must be stashed"
    );
    assert_eq!(
        fx.turn_model.requests().len(),
        0,
        "a regenerable output costs no notes call"
    );
    Ok(())
}

/// WHEN the policy's `near_window` is `Clear`, THEN THE decision SHALL clear
/// silently whatever the outputs are: no notes call and no notes in the patch.
#[tokio::test]
async fn decide_compaction_clears_silently_when_the_policy_says_clear() -> Result<()> {
    // -- Setup & Fixtures: the untracked output that asks for notes under
    // `Summarize`, and a policy that says `Clear`.
    let mut policy = clearing_policy();
    policy.near_window = NearWindow::Clear;
    let fx = decide_fixture_with_policy(policy)?;
    let spec = spec_with_input_limit(60_000)?;
    seed_rig(&fx, &[result_turn("tc-1", 200_000)?]).await?;

    // -- Exec
    let action = decide_compaction(
        &Message::user("hi"),
        "conv-1",
        InPlace {
            spec: Some(&spec),
            pending: &fx.pending,
        },
        &CompactionPath::new(
            fx.memory.as_ref(),
            &fx.compaction,
            &fx.meter,
            &Default::default(),
            &fx.model,
        ),
        fx.bus.stream(),
        fx.bus.lifecycle(),
    )
    .await;

    // -- Check
    let sent = patched_history(&action)
        .ok_or_else(|| format!("the clearing must patch this call, got {action:?}"))?;
    assert!(
        sent.iter()
            .any(|message| result_text(message) == ClearToolOutputs::PLACEHOLDER),
        "the patch must send the placeholder instead of the cleared output"
    );
    assert!(
        sent.iter()
            .all(|message| !user_text(message).contains("<cleared-output-notes>")),
        "a `Clear` policy asks for no notes"
    );
    assert!(
        fx.pending.take_clear().is_some(),
        "the clearing must be stashed"
    );
    assert_eq!(
        fx.turn_model.requests().len(),
        0,
        "a `Clear` policy costs no notes call"
    );
    Ok(())
}

/// WHEN a clearing would leave more than half of `limit - reserve` used, THEN THE
/// decision SHALL skip it and take the side-call: the summary on its own, stashed
/// for the executor, with nothing cleared.
#[tokio::test]
async fn decide_compaction_clearing_that_frees_too_little_takes_the_side_call() -> Result<()> {
    // -- Setup & Fixtures: ~50k tokens of conversation whose only clearable output
    // is ~1k tokens — far too little of the 48_000 usable, so the checkpoint rule
    // refuses it.
    let fx = decide_fixture_with_policy(clearing_policy())?;
    let spec = spec_with_input_limit(60_000)?;
    seed_rig(
        &fx,
        &[
            Message::user("an older question"),
            Message::user("q".repeat(200_000)),
            call_turn(read_call("tc-1", "/tmp/whatever")?),
            result_turn("tc-1", 4_000)?,
        ],
    )
    .await?;
    let mut control_rx = fx.bus.control().subscribe();

    // -- Exec
    let action = decide_compaction(
        &Message::user("hi"),
        "conv-1",
        InPlace {
            spec: Some(&spec),
            pending: &fx.pending,
        },
        &CompactionPath::new(
            fx.memory.as_ref(),
            &fx.compaction,
            &fx.meter,
            &Default::default(),
            &fx.model,
        ),
        fx.bus.stream(),
        fx.bus.lifecycle(),
    )
    .await;

    // -- Check: no patch, no clearance, and the side-call's summary waits.
    assert!(
        matches!(action, rig::agent::CompletionCallAction::Continue),
        "a clearing that frees too little must not patch, got {action:?}"
    );
    assert!(
        fx.pending.take_clear().is_none(),
        "a refused clearing must stash no clearance"
    );
    assert!(
        fx.pending.take_settled().is_some(),
        "the side-call's summary must wait for the executor"
    );
    assert!(
        matches!(
            control_rx.try_recv(),
            Err(crate::bus::TryReceiveError::Empty)
        ),
        "auto compaction SHALL NOT schedule a worker compaction"
    );
    assert_eq!(
        fx.turn_model.requests().len(),
        1,
        "the side-call asks the turn's own model exactly once"
    );
    Ok(())
}
