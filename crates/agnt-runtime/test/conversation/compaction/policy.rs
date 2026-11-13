//! `build_compaction_policy()`: the `[compaction]` block into a rig-memory policy.

use crate::config::CompactionConfig;
use crate::conversation::compaction::compactor::resolve_model_spec;
use crate::conversation::runtime_build::build_compaction_policy;
use crate::types::{
    AssistantContent, AssistantMessage, CallId, Message, Text, ToolCall, ToolFunction, ToolName,
    ToolResult, ToolResultContent, UserContent,
};

use rig::catalog::ModelSpec;
use rig_memory::{
    ClearToolOutputs, CompactReason, CompactionPolicy, MemoryStore, SummaryPrompts, TrackArgument,
};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn test_conversation_compaction_policy_nothing_set_pins_summary_tokens() -> Result<()> {
    // -- Setup & Fixtures
    let config = CompactionConfig::default();
    let mut expected = CompactionPolicy::default();
    // The 32k budget is our deliberate pin (c5t 7eb115fe), not rig's default,
    // and the tracked rules are ours: read's path and edit's path.
    expected.summarizer.limits.summary_tokens = 32_000;
    expected.tracked = our_tracked_rules();

    // -- Exec
    let policy = build_compaction_policy(&config);

    // -- Check
    assert_eq!(policy, expected);
    Ok(())
}

#[test]
fn test_conversation_compaction_policy_summary_tokens_override_wins() -> Result<()> {
    // -- Setup & Fixtures
    let config = CompactionConfig {
        summary_tokens: Some(6_000),
        ..CompactionConfig::default()
    };

    // -- Exec
    let policy = build_compaction_policy(&config);

    // -- Check
    assert_eq!(policy.summarizer.limits.summary_tokens, 6_000);
    Ok(())
}

#[test]
fn test_conversation_compaction_policy_each_setting_is_carried() -> Result<()> {
    // -- Setup & Fixtures
    let config = CompactionConfig {
        summary_tokens: Some(6_000),
        snippet_chars: Some(1_500),
        unknown_window_chars: Some(200_000),
        keep_recent: Some(12_000),
        keep_asked: Some(2_000),
        clearing: Some(ClearToolOutputs {
            keep_tokens: 9_000,
            keep_under: 250,
        }),
        tracked: Some(vec![TrackArgument::new("read", "path", "files_read")]),
        prompts: Some(SummaryPrompts {
            system: "system prompt".into(),
            ..Default::default()
        }),
        ..CompactionConfig::default()
    };

    // -- Exec
    let policy = build_compaction_policy(&config);

    // -- Check
    assert_eq!(policy.summarizer.limits.summary_tokens, 6_000);
    assert_eq!(policy.summarizer.limits.snippet_chars, 1_500);
    assert_eq!(policy.summarizer.limits.unknown_window_chars, 200_000);
    assert_eq!(policy.keep_recent, 12_000);
    assert_eq!(policy.keep_asked, 2_000);
    assert_eq!(policy.clearing.keep_tokens, 9_000);
    assert_eq!(policy.clearing.keep_under, 250);
    assert_eq!(
        policy.tracked,
        vec![TrackArgument::new("read", "path", "files_read")]
    );
    assert_eq!(policy.summarizer.prompts.system, "system prompt");
    // The prompts left unset keep rig's default text.
    assert_eq!(
        policy.summarizer.prompts.update,
        SummaryPrompts::DEFAULT.update
    );
    // `near_window` is not a config key: it stays rig's default.
    assert_eq!(policy.near_window, CompactionPolicy::default().near_window);
    Ok(())
}

#[test]
fn test_conversation_compaction_policy_unset_fields_keep_default_except_summary_budget()
-> Result<()> {
    // -- Setup & Fixtures
    let config = CompactionConfig {
        keep_recent: Some(5_000),
        ..CompactionConfig::default()
    };
    let default = CompactionPolicy::default();
    let mut expected_summarizer = default.summarizer.clone();
    // The 32k budget is our deliberate pin (c5t 7eb115fe), not rig's default.
    expected_summarizer.limits.summary_tokens = 32_000;

    // -- Exec
    let policy = build_compaction_policy(&config);

    // -- Check
    assert_eq!(policy.keep_recent, 5_000);
    assert_eq!(policy.keep_asked, default.keep_asked);
    assert_eq!(policy.clearing, default.clearing);
    assert_eq!(policy.summarizer, expected_summarizer);
    assert_eq!(policy.tracked, our_tracked_rules());
    assert_eq!(policy.near_window, default.near_window);
    Ok(())
}

/// WHEN the block leaves `tracked` unset, THEN THE build SHALL return exactly
/// our two rules: `read`'s path and `edit`'s path.
#[test]
fn test_conversation_compaction_policy_absent_tracked_yields_our_two_rules() -> Result<()> {
    // -- Setup & Fixtures
    let config = CompactionConfig::default();

    // -- Exec
    let policy = build_compaction_policy(&config);

    // -- Check
    assert_eq!(
        policy.tracked,
        vec![
            TrackArgument::new("read", "path", "read-files"),
            TrackArgument::new("edit", "path", "modified-files"),
        ],
        "the built-in rules are the two our file tools need"
    );
    Ok(())
}

/// WHEN the block sets `tracked`, THEN THE build SHALL return exactly what it
/// set — an explicit empty list included.
#[test]
fn test_conversation_compaction_policy_explicit_tracked_wins_verbatim() -> Result<()> {
    // -- Setup & Fixtures: one rule, then an explicit empty list.
    let one = TrackArgument::new("grep", "pattern", "grepped");
    let configured = CompactionConfig {
        tracked: Some(vec![one.clone()]),
        ..CompactionConfig::default()
    };
    let emptied = CompactionConfig {
        tracked: Some(Vec::new()),
        ..CompactionConfig::default()
    };

    // -- Exec
    let policy = build_compaction_policy(&configured);
    let empty_policy = build_compaction_policy(&emptied);

    // -- Check
    assert_eq!(policy.tracked, vec![one], "an explicit list wins verbatim");
    assert!(
        empty_policy.tracked.is_empty(),
        "an explicit empty list turns tracking off, got {:?}",
        empty_policy.tracked
    );
    Ok(())
}

/// WHEN the block sets `near_window`, THEN THE build SHALL return that variant;
/// WHEN it is absent, THE build SHALL keep rig-memory's default.
#[test]
fn test_conversation_compaction_policy_near_window_key() -> Result<()> {
    // -- Setup & Fixtures
    let cleared = CompactionConfig {
        near_window: Some(rig_memory::NearWindow::Clear),
        ..CompactionConfig::default()
    };
    let absent = CompactionConfig::default();

    // -- Exec
    let cleared_policy = build_compaction_policy(&cleared);
    let absent_policy = build_compaction_policy(&absent);

    // -- Check
    assert_eq!(cleared_policy.near_window, rig_memory::NearWindow::Clear);
    assert_eq!(
        absent_policy.near_window,
        CompactionPolicy::default().near_window,
        "an absent key keeps rig-memory's default"
    );
    Ok(())
}

/// The tracked sets record only the arguments of successful tool calls:
/// `SummaryState::track` skips every result with `is_error` set
/// (rig-memory-0.46.0 `src/compaction.rs:124-160`), so a failed read's path is
/// not carried into the summary.
#[test]
fn test_conversation_compaction_policy_tracked_set_excludes_error_results() -> Result<()> {
    // -- Setup & Fixtures
    let config = CompactionConfig {
        keep_recent: Some(0),
        tracked: Some(vec![TrackArgument::new("read", "path", "files_read")]),
        ..CompactionConfig::default()
    };
    let policy = build_compaction_policy(&config);
    let store = MemoryStore::default();
    let mut conversation = rig_memory::load(&store, "conv-1")?;
    conversation.extend([
        Message::Assistant(AssistantMessage::new(vec![
            AssistantContent::ToolCall(read_call("call-ok", "/tmp/read.txt")?),
            AssistantContent::ToolCall(read_call("call-err", "/tmp/missing.txt")?),
        ])),
        Message::User {
            content: vec![
                UserContent::ToolResult(read_result("call-ok", "/tmp/read.txt", false)?),
                UserContent::ToolResult(read_result("call-err", "/tmp/missing.txt", true)?),
            ],
        },
        Message::assistant("done"),
    ]);
    let spec = test_spec()?;

    // -- Exec
    let plan = policy
        .plan_summary(
            &conversation,
            &rig_memory::summary_state(&conversation),
            &CompactReason::Threshold,
            &spec,
        )?
        .ok_or("the conversation has older messages to summarize")?;

    // -- Check
    let tracked = plan
        .state
        .tracked
        .iter()
        .find(|set| set.name == "files_read")
        .ok_or("the rule creates its tracked set")?;
    assert_eq!(
        tracked
            .values
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["/tmp/read.txt"],
        "only the successful call's path is tracked; the failed call's path is not"
    );
    Ok(())
}

// region:    --- Test Support

/// Our built-in tracked rules: the path arguments a summary keeps.
fn our_tracked_rules() -> Vec<TrackArgument> {
    vec![
        TrackArgument::new("read", "path", "read-files"),
        TrackArgument::new("edit", "path", "modified-files"),
    ]
}

/// A registered provider with a window wide enough for the summarizer's
/// default reserve and summary budget.
fn test_spec() -> Result<ModelSpec> {
    resolve_model_spec("openai", "gpt-4o", Some(200_000), Some(1_024), None)
        .ok_or_else(|| "openai should resolve to a registered provider id".into())
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

/// The result answering `call` for `path`, marked failed when `is_error`.
fn read_result(call: &str, path: &str, is_error: bool) -> Result<ToolResult> {
    Ok(ToolResult {
        call: CallId::from_wire(call),
        name: ToolName::new("read").map_err(|_| "read is a non-empty tool name")?,
        content: vec![ToolResultContent::Text(Text::new(path))],
        is_error,
    })
}

// endregion: --- Test Support
