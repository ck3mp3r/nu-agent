use std::fs;
use std::path::Path;

fn read_help_markdown() -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/agent/ui/tui/runtime/help/help.md"))
        .expect("help markdown")
}

fn read_usage_docs() -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/usage.md"))
        .expect("usage docs")
}

#[test]
fn help_includes_compact_mcp_help_status_slash_commands() {
    let help = read_help_markdown();
    assert!(help.contains("`/compact`"));
    assert!(help.contains("`/mcp`"));
    assert!(help.contains("`/help`"));
    assert!(help.contains("`/status`"));
    assert!(help.contains("`/models`"));
}

#[test]
fn help_describes_auto_compaction_threshold_behavior() {
    let help = read_help_markdown();
    assert!(help.contains("threshold - tolerance"));
    assert!(help.contains("threshold - (tolerance + hysteresis_margin)"));
}

#[test]
fn help_describes_force_compact_behavior() {
    let help = read_help_markdown();
    assert!(help.contains("force compaction"));
    assert!(help.contains("bypasses threshold gating"));
}

#[test]
fn docs_do_not_claim_skills_listing_tool_for_slash_commands() {
    let docs = read_usage_docs();
    let lowered = docs.to_ascii_lowercase();
    assert!(lowered.contains("/compact"));
    assert!(!lowered.contains("skills listing tool"));
}

#[test]
fn docs_describe_session_memory_persistence_after_compaction() {
    let docs = read_usage_docs();
    assert!(docs.contains("session JSONL"));
    assert!(docs.contains("compaction_count"));
}

#[test]
fn help_describes_inline_slash_suggestions_behavior() {
    let help = read_help_markdown();
    assert!(help.contains("Inline slash suggestions"));
    assert!(help.contains("open when input starts with `/`"));
    assert!(help.contains("filters incrementally"));
    assert!(help.contains("independent of the command palette"));
}

#[test]
fn help_describes_sliding_summary_only_compaction() {
    let help = read_help_markdown();
    assert!(help.contains("sliding_summary"));
    assert!(help.contains("single active compaction mode"));
}

#[test]
fn docs_describe_transcript_visible_compaction_summary() {
    let docs = read_usage_docs();
    assert!(docs.contains("transcript-visible compaction summary"));
    assert!(docs.contains("source + summarized/kept counts"));
}

#[test]
fn help_describes_no_transcript_echo_for_slash_commands() {
    let help = read_help_markdown();
    assert!(help.contains("not echoed into the transcript"));
    assert!(help.contains("only resulting artifacts"));
}

#[test]
fn help_includes_models_slash_command() {
    let help = read_help_markdown();
    assert!(help.contains("`/models`"));
    assert!(help.contains("opens the inline model picker"));
}

#[test]
fn help_describes_models_launcher_via_command_palette() {
    let help = read_help_markdown();
    assert!(help.contains("Ctrl-P `Models`"));
    assert!(help.contains("same picker path"));
}

#[test]
fn help_describes_model_switch_uses_cached_startup_config() {
    let help = read_help_markdown();
    assert!(help.contains("cached startup `PluginConfig`"));
    assert!(help.contains("no per-switch plugin config re-read"));
}

#[test]
fn help_describes_modal_rounded_border_and_dim_backdrop() {
    let help = read_help_markdown();
    assert!(help.contains("rounded borders"));
    assert!(help.contains("dimmed backdrop"));
}

#[test]
fn usage_docs_describe_edit_mode_preview_apply_contract_and_envelope_fields() {
    let docs = read_usage_docs();
    assert!(docs.contains("canonical contract (preview/apply)"));
    assert!(docs.contains("\"mode\": \"preview\""));
    assert!(docs.contains("`mode: \"apply\"`"));
    assert!(docs.contains("proposal_id"));
    assert!(docs.contains("would_change"));
    assert!(docs.contains("diagnostics"));
}

#[test]
fn usage_docs_list_deterministic_edit_error_taxonomy_classes() {
    let docs = read_usage_docs();
    assert!(docs.contains("validation"));
    assert!(docs.contains("stale"));
    assert!(docs.contains("permission"));
    assert!(docs.contains("conflict"));
    assert!(docs.contains("internal"));
}
