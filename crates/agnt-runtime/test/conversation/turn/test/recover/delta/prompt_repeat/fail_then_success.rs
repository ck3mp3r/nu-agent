//! The fail-then-success pin: a turn that persisted a failed attempt's exchange
//! and then succeeded holds one prompt row.

use super::*;

/// A turn whose failed attempt persisted its exchange and whose later attempt
/// succeeded holds ONE prompt row: the mid-flight delta carried no prompt, and
/// rig's own done-arm save carries the run's request once.
#[tokio::test]
async fn test_persist_turn_delta_fail_then_success_holds_one_prompt() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config::default();
    let mut memory_state = MemoryState::new(RigStoreConfig::in_memory());
    let session_id = "test-recover-fail-then-success";
    let full_history = vec![
        Message::user("q2"),
        assistant_tool_calls(&["tc1"])?,
        Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: CallId::from_wire("tc1"),
                name: ToolName::new("read_file")?,
                content: vec![ToolResultContent::text("ok")],
                is_error: false,
            })],
        },
    ];
    let records = vec![record("tc1", Some("ok"))?];

    // -- Exec: the failed attempt's mid-flight persist, prompt-free.
    executor(&config, &mut memory_state)
        .persist_turn_delta(
            Some(session_id),
            "q2",
            report(&full_history, dispatched_after(0)),
            0,
            PersistStage::MidFlight { keep_head: false },
            recovery(&records, None),
        )
        .await;

    // The retry succeeds: rig's done arm extends the loaded conversation with the
    // run's own committed messages, the prompt and its reply.
    {
        let store = memory_state.inner_memory().rig_store(session_id).await?;
        let mut conversation = rig_memory::load(&*store, session_id)?;
        conversation.extend(vec![Message::user("q2"), Message::assistant("done")]);
        rig_memory::save(&*store, session_id, &mut conversation)?;
    }

    // -- Check
    let messages = stored(&memory_state, session_id).await?;
    assert_eq!(
        prompt_rows(&messages, "q2"),
        1,
        "the turn holds exactly one prompt row; got {messages:?}"
    );
    Ok(())
}

/// The user rows whose text is `text`, counting a row rig's repair merged with
/// its neighbours as one prompt-form row.
fn prompt_rows(messages: &[Message], text: &str) -> usize {
    messages
        .iter()
        .filter_map(|message| match message {
            Message::User { content } => Some(content),
            _ => None,
        })
        .filter(|content| {
            let blocks: String = content
                .iter()
                .filter_map(|item| match item {
                    UserContent::Text(block) => Some(block.text.as_str()),
                    _ => None,
                })
                .collect();
            blocks == text
        })
        .count()
}
