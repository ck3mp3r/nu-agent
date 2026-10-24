use super::*;

use rig::conversation::pairing::{NO_RESULT_PROVIDED, repair};

/// A result row the store already holds is not written a second time.
///
/// rig's cancellation repair merges adjacent user messages, so the prompt the
/// next turn reports carries the store's last result row. Persisting that row
/// again would hold one call's result twice, and a provider that rejects a
/// repeated tool-result id fails the first turn after a reload.
#[tokio::test]
async fn persist_turn_delta_does_not_repeat_a_stored_result() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config::default();
    let mut memory_state = MemoryState::new(RigStoreConfig::in_memory());
    let session_id = "test-recover-stored-result";
    let call = CallId::from_wire("tc1");
    // The store after an interrupted turn: the dangling call closed by rig's
    // placeholder result.
    let interrupted = vec![
        Message::user("q1"),
        assistant_tool_calls(&["tc1"])?,
        Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: call.clone(),
                name: ToolName::new("read_file")?,
                content: vec![ToolResultContent::text(NO_RESULT_PROVIDED)],
                is_error: true,
            })],
        },
    ];
    seed(&memory_state, session_id, interrupted.clone())?;
    // The next turn reports rig's canonical history: the repair merges the
    // store's last result row into the turn's prompt.
    let mut reported = interrupted;
    reported.push(Message::user("q2"));
    reported.push(Message::assistant("partial"));
    let full_history = repair(reported).messages;

    // -- Exec
    executor(&config, &mut memory_state)
        .persist_turn_delta(
            Some(session_id),
            "q2",
            &full_history,
            3,
            INTERRUPTED_REPLY,
            recovery(&[], None),
        )
        .await;

    // -- Check
    let messages = stored(&memory_state, session_id)?;
    assert_eq!(
        result_rows(&messages, &call),
        1,
        "one result row per call id; got {messages:?}"
    );
    assert!(
        messages.contains(&Message::user("q2")),
        "the prompt is persisted; got {messages:?}"
    );
    Ok(())
}

/// How many result rows in `messages` answer `call`.
fn result_rows(messages: &[Message], call: &CallId) -> usize {
    messages
        .iter()
        .filter_map(|message| match message {
            Message::User { content } => Some(content),
            _ => None,
        })
        .flatten()
        .filter(|item| matches!(item, UserContent::ToolResult(result) if result.call == *call))
        .count()
}
