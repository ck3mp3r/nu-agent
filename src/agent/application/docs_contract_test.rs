use std::fs;
use std::path::Path;

fn read_help_markdown() -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/agent/ui/tui/runtime/help/help.md"),
    )
    .expect("help markdown")
}

fn read_usage_docs() -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/usage.md"))
        .expect("usage docs")
}

fn read_contribution_guardrails_docs() -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/contribution-guardrails.md"),
    )
    .expect("contribution guardrails docs")
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

#[test]
fn usage_docs_describe_permissions_dsl_canonical_shape_and_precedence() {
    let docs = read_usage_docs();
    assert!(docs.contains("Only CLI surface for policy override is `--permissions`"));
    assert!(!docs.contains("--permission "));
    assert!(docs.contains("map-style"));
    assert!(docs.contains("permissions DSL"));
    assert!(
        docs.contains(
            "\"nu__run\": { \"command\": { \"kubectl delete *\": \"deny\", \"*\": \"ask\" } }"
        ) || docs.contains("\"nu__run\": {")
            && docs.contains("\"command\": {")
            && docs.contains("\"kubectl delete *\": \"deny\"")
    );
    assert!(docs.contains("global baseline"));
    assert!(docs.contains("tool override"));
    assert!(docs.contains("nested `nu__run.command` override"));
}

#[test]
fn usage_docs_describe_permissions_overlay_startup_diagnostics() {
    let docs = read_usage_docs();
    assert!(docs.contains("overlay_active=true|false"));
    assert!(docs.contains("permissions policy:"));
}

#[test]
fn usage_docs_describe_permissions_ask_choices_and_session_grants() {
    let docs = read_usage_docs();
    assert!(docs.contains("allow_once"));
    assert!(docs.contains("allow_always"));
    assert!(docs.contains("session-only"));
    assert!(docs.contains("reset on restart"));
    assert!(docs.contains("not global across unrelated tools"));
    assert!(docs.contains("same scoped tool context"));
}

#[test]
fn usage_docs_describe_permission_modal_keybindings_and_lifecycle_events() {
    let docs = read_usage_docs();
    assert!(docs.contains("Interactive permission prompt behavior"));
    assert!(docs.contains("`a` => `allow_once`"));
    assert!(docs.contains("`A` => `allow_always`"));
    assert!(docs.contains("`d` => `deny`"));
    assert!(docs.contains("`Esc` => `deny`"));
    assert!(docs.contains("PermissionRequested"));
    assert!(docs.contains("PermissionDecisionSubmitted"));
    assert!(docs.contains("PermissionDecisionTimedOut"));
    assert!(docs.contains("PermissionDecisionIgnored"));
}

#[test]
fn usage_docs_describe_non_interactive_ask_default_and_override() {
    let docs = read_usage_docs();
    assert!(docs.contains("Non-interactive ask fallback"));
    assert!(docs.contains("non_interactive_ask"));
    assert!(docs.contains("default (missing): `deny`"));
    assert!(docs.contains("supported values: `deny`, `allow`"));
}

#[test]
fn contribution_guardrails_doc_links_handler_contract_and_usage_sections() {
    let docs = read_contribution_guardrails_docs();
    assert!(docs.contains("./handler-decomposition-contract.md"));
    assert!(docs.contains("./usage.md#interactive-permission-prompt-behavior-tui"));
}

#[test]
fn contribution_guardrails_doc_references_key_runtime_and_handler_tests() {
    let docs = read_contribution_guardrails_docs();
    assert!(docs.contains("src/agent/ui/tui/runtime/mod.rs"));
    assert!(docs.contains("src/agent/ui/tui/runtime/test.rs"));
    assert!(docs.contains("src/agent/tools/handler/test.rs"));
    assert!(docs.contains("src/agent/protocol/permission_test.rs"));
}
