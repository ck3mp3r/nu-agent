use super::*;

use crate::types::{CallId, ToolName, ToolResult, ToolResultContent};
use rig_memory::ClearToolOutputs;

/// A policy that clears every tool output older than the newest message, so the
/// summarizer reads the placeholder instead of the raw output.
fn clearing_policy() -> CompactionPolicy {
    CompactionPolicy {
        keep_recent: 0,
        keep_asked: 0,
        clearing: ClearToolOutputs::new(0),
        ..CompactionPolicy::default()
    }
}

/// A tool result whose output is far over `ClearToolOutputs::keep_under`.
fn big_tool_result(output: &str) -> Result<UserContent> {
    Ok(UserContent::ToolResult(ToolResult {
        call: CallId::from_wire("call-1"),
        name: ToolName::new("read_file").map_err(|_| "read_file is a non-empty tool name")?,
        content: vec![ToolResultContent::text(output)],
        is_error: false,
    }))
}

#[tokio::test]
async fn compact_clears_old_tool_outputs_before_the_summary() -> Result<()> {
    // -- Setup & Fixtures
    // The old tool output is far over `keep_under`, so the policy clears it. The
    // summarizer must read the placeholder, not the raw output.
    let store = memory_store()?;
    let raw_output = "raw tool output ".repeat(400);
    let mut conversation = rig_memory::load(&*store, CONV)?;
    conversation.extend([
        Message::User {
            content: vec![big_tool_result(&raw_output)?],
        },
        Message::assistant("read the file"),
        Message::user("what next?"),
        Message::assistant("next answer"),
    ]);
    rig_memory::save(&*store, CONV, &mut conversation)?;

    let bus = Bus::default();
    let model = stream_model(&["summary after clearing"]);
    let compactor = NuCompactor::new(
        model.clone().erase(),
        bus,
        clearing_policy(),
        Some(test_spec()?),
    );

    // -- Exec
    let outcome = compactor
        .compact(&*store, CONV, &CompactReason::Threshold, "auto")
        .await?;

    // -- Check
    assert!(
        outcome.is_some(),
        "the conversation is over the summarizer reserve, so it must compact"
    );
    let prompt = last_request_prompt(&model)?;
    assert!(
        prompt.contains(ClearToolOutputs::PLACEHOLDER),
        "the summarizer must read the cleared placeholder, got: {prompt}"
    );
    assert!(
        !prompt.contains("raw tool output"),
        "the summarizer must not read the raw tool output, got: {prompt}"
    );
    Ok(())
}

#[tokio::test]
async fn compact_keeps_tool_outputs_the_policy_does_not_clear() -> Result<()> {
    // -- Setup & Fixtures
    // The default policy keeps the newest 20k tokens of output, so a small
    // conversation's output is never cleared.
    let store = memory_store()?;
    let mut conversation = rig_memory::load(&*store, CONV)?;
    conversation.extend([
        Message::User {
            content: vec![big_tool_result("raw tool output ".repeat(400).as_str())?],
        },
        Message::assistant("read the file"),
        Message::user("what next?"),
        Message::assistant("next answer"),
    ]);
    rig_memory::save(&*store, CONV, &mut conversation)?;

    let bus = Bus::default();
    let model = stream_model(&["summary"]);
    let compactor = NuCompactor::new(
        model.clone().erase(),
        bus,
        test_policy(),
        Some(test_spec()?),
    );

    // -- Exec
    compactor
        .compact(&*store, CONV, &CompactReason::Threshold, "auto")
        .await?;

    // -- Check
    let prompt = last_request_prompt(&model)?;
    assert!(
        prompt.contains("raw tool output"),
        "an output the policy keeps must reach the summarizer, got: {prompt}"
    );
    assert!(
        !prompt.contains(ClearToolOutputs::PLACEHOLDER),
        "nothing may be cleared when the policy keeps every output, got: {prompt}"
    );
    Ok(())
}
