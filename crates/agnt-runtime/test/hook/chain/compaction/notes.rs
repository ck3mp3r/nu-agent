//! The notes a near-window clearing asks for: when they are needed, what the
//! request carries, and when there is no room to ask.

use super::*;

use rig_memory::{ClearToolOutputs, MemoryStore, NearWindow, TrackArgument};

use crate::types::{
    AssistantContent, AssistantMessage, CallId, Message, ToolCall, ToolFunction, ToolName,
    ToolResult, ToolResultContent, UserContent,
};

// region:    --- Test Support

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// The policy the notes rules read: every output above one token is clearable.
fn clearing_policy() -> CompactionPolicy {
    CompactionPolicy {
        clearing: ClearToolOutputs {
            keep_tokens: 0,
            keep_under: 1,
        },
        ..CompactionPolicy::default()
    }
}

/// The policy that tracks our `read` tool's path argument.
fn tracked_policy() -> CompactionPolicy {
    CompactionPolicy {
        tracked: vec![TrackArgument::new("read", "path", "read-files")],
        ..clearing_policy()
    }
}

/// A `read` call of `path`.
fn read_call(call: &str, path: &str) -> Result<ToolCall> {
    Ok(ToolCall::new(
        CallId::from_wire(call),
        ToolFunction::new(
            ToolName::new("read").map_err(|_| "read is a non-empty tool name")?,
            serde_json::json!({ "path": path }),
        ),
    ))
}

/// A conversation holding the `read` call of `path` and its output: 4k
/// characters of text.
fn conversation_reading(path: &str) -> Result<rig::conversation::Conversation> {
    let name = ToolName::new("read").map_err(|_| "read is a non-empty tool name")?;
    let result = ToolResult {
        call: CallId::from_wire("tc-1"),
        name: name.clone(),
        content: vec![ToolResultContent::text("o".repeat(4_000))],
        is_error: false,
    };
    let store = MemoryStore::default();
    let mut conversation = rig_memory::load(&store, "conv-1")?;
    conversation.extend([
        Message::user("the turn's prompt"),
        Message::Assistant(AssistantMessage::new(vec![AssistantContent::ToolCall(
            read_call("tc-1", path)?,
        )])),
        Message::User {
            content: vec![UserContent::ToolResult(result)],
        },
        Message::assistant("the run's answer"),
    ]);
    Ok(conversation)
}

/// A model spec whose input limit is `tokens`.
fn spec_with_input_limit(tokens: u32) -> Result<rig::catalog::ModelSpec> {
    crate::conversation::compaction::compactor::resolve_model_spec(
        "openai",
        "gpt-4o",
        Some(tokens),
        Some(1_024),
        None,
    )
    .ok_or_else(|| "openai must resolve to a registered provider id".into())
}

/// The user text of `message`, empty for any other message.
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

// endregion: --- Test Support

/// WHEN the outputs a clearing drops cannot be had again, THEN THE clearing SHALL
/// need notes; WHEN every one of them can, it SHALL clear silently.
#[test]
fn test_clear_notes_needed_only_for_an_output_that_cannot_be_had_again() -> Result<()> {
    // -- Setup & Fixtures: the same conversation, once with an output whose path
    // still stats and once with a path that does not.
    let temp_dir = tempfile::TempDir::new().map_err(|_| "should create temp dir")?;
    let path = temp_dir.path().join("notes.txt");
    std::fs::write(&path, b"the file the call read").map_err(|_| "should write the file")?;
    let existing = path.to_str().ok_or("the temp path is UTF-8")?.to_string();
    let policy = tracked_policy();

    // -- Exec
    let regenerable = conversation_reading(&existing)?;
    let gone = conversation_reading("/definitely/not/here.txt")?;
    let untracked = conversation_reading(&existing)?;

    // -- Check
    assert!(
        !notes_needed(&policy, &regenerable, &policy.clearing.plan(&regenerable)),
        "a tracked output whose path still stats needs no notes"
    );
    assert!(
        notes_needed(&policy, &gone, &policy.clearing.plan(&gone)),
        "a tracked output whose path is gone needs notes"
    );
    assert!(
        notes_needed(
            &clearing_policy(),
            &untracked,
            &clearing_policy().clearing.plan(&untracked)
        ),
        "an output of an untracked tool needs notes"
    );
    Ok(())
}

/// WHEN the policy's `near_window` is `Clear`, THEN THE clearing SHALL need no
/// notes whatever its outputs are.
#[test]
fn test_clear_notes_needed_false_when_the_policy_says_clear() -> Result<()> {
    // -- Setup & Fixtures: an untracked output, which needs notes under
    // `Summarize`.
    let mut policy = clearing_policy();
    policy.near_window = NearWindow::Clear;
    let conversation = conversation_reading("/definitely/not/here.txt")?;

    // -- Check
    assert!(
        !notes_needed(&policy, &conversation, &policy.clearing.plan(&conversation)),
        "a `Clear` policy clears silently"
    );
    Ok(())
}

/// WHEN the room below the model's input limit leaves the output floor, THEN THE
/// notes request SHALL ask for the notes instruction at that room's budget, after
/// the conversation and the summarizer's system prompt.
#[test]
fn test_clear_notes_request_asks_the_instruction_at_the_room_budget() -> Result<()> {
    // -- Setup & Fixtures
    let policy = clearing_policy();
    let conversation = conversation_reading("/tmp/whatever")?;
    let spec = spec_with_input_limit(60_000)?;
    let tokens = 10_000;

    // -- Exec
    let request = notes_request(&policy, &spec, &conversation, tokens)
        .ok_or("a 50k room is well above the output floor")?;

    // -- Check: the room is the budget, the system prompt leads, and the
    // instruction asks last, over the whole conversation.
    assert_eq!(request.max_tokens, Some(50_000), "the budget is the room");
    assert_eq!(
        request.chat_history.len(),
        conversation.messages().len() + 2,
        "the request carries the conversation, the system prompt, and the instruction"
    );
    let Some(Message::System { content }) = request.chat_history.first().map(|m| (**m).clone())
    else {
        return Err("the summarizer's system prompt must lead the request".into());
    };
    assert_eq!(
        content,
        policy.summarizer.prompts.system.as_ref(),
        "the request carries the summarizer's system prompt"
    );
    let instruction = request
        .chat_history
        .last()
        .ok_or("the request carries the instruction")?;
    assert_eq!(
        user_text(instruction),
        policy.summarizer.notes_instruction(50_000),
        "the request asks for the notes instruction at the budget"
    );
    Ok(())
}

/// WHEN the room below the model's input limit is less than the least an answer
/// is worth, THEN THE notes request SHALL ask nothing.
#[test]
fn test_clear_notes_request_without_the_output_floor_asks_nothing() -> Result<()> {
    // -- Setup & Fixtures: a 5k limit with 1k tokens already sent leaves a 4k
    // room, below the 4_096 floor.
    let policy = clearing_policy();
    let conversation = conversation_reading("/tmp/whatever")?;
    let spec = spec_with_input_limit(5_000)?;

    // -- Exec
    let request = notes_request(&policy, &spec, &conversation, 1_000);

    // -- Check
    assert!(
        request.is_none(),
        "a request that cannot fit asks for nothing"
    );
    Ok(())
}

/// WHEN the tracked path is gone from this machine, THEN THE output SHALL not
/// count as one that can be had again.
#[test]
fn test_clear_all_again_false_when_the_tracked_path_is_gone() -> Result<()> {
    // -- Setup & Fixtures
    let policy = tracked_policy();
    let conversation = conversation_reading("/definitely/not/here.txt")?;
    let plan = policy.clearing.plan(&conversation);

    // -- Exec
    let again = all_again(&conversation, &plan, &policy.tracked);

    // -- Check
    assert!(!again, "a path that no longer stats cannot be re-read");
    assert_eq!(
        plan.cleared.results, 1,
        "the plan must name the output it drops"
    );
    Ok(())
}
