//! `build_compaction_policy()`: the `[compaction]` block into a rig-memory policy.

use crate::config::CompactionConfig;
use crate::conversation::runtime_build::build_compaction_policy;

use rig_memory::{ClearToolOutputs, CompactionPolicy, SummaryPrompts, TrackArgument};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn test_conversation_compaction_policy_nothing_set_is_rig_default() -> Result<()> {
    // -- Setup & Fixtures
    let config = CompactionConfig::default();

    // -- Exec & Check
    assert_eq!(
        build_compaction_policy(&config),
        CompactionPolicy::default()
    );
    Ok(())
}

#[test]
fn test_conversation_compaction_policy_each_setting_is_carried() -> Result<()> {
    // -- Setup & Fixtures
    let config = CompactionConfig {
        reserve: Some(8_000),
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
    assert_eq!(policy.summarizer.limits.reserve, 8_000);
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
fn test_conversation_compaction_policy_unset_fields_keep_rig_default() -> Result<()> {
    // -- Setup & Fixtures
    let config = CompactionConfig {
        keep_recent: Some(5_000),
        ..CompactionConfig::default()
    };
    let default = CompactionPolicy::default();

    // -- Exec
    let policy = build_compaction_policy(&config);

    // -- Check
    assert_eq!(policy.keep_recent, 5_000);
    assert_eq!(policy.keep_asked, default.keep_asked);
    assert_eq!(policy.clearing, default.clearing);
    assert_eq!(policy.summarizer, default.summarizer);
    assert_eq!(policy.tracked, default.tracked);
    assert_eq!(policy.near_window, default.near_window);
    Ok(())
}
