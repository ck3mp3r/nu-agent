use super::test_helpers::{
    create_test_agent, first_unknown_flag_error, parse_agent_invocation_with_signature,
};
use nu_plugin::SimplePluginCommand;
use nu_protocol::SyntaxShape;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn agent_command_has_correct_name() {
    let (agent, _temp_dir) = create_test_agent();
    assert_eq!(SimplePluginCommand::name(&agent), "agent");
}

#[test]
fn agent_command_signature_accepts_string() {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);

    // Verify the command name
    assert_eq!(sig.name, "agent");
}

#[test]
fn agent_command_signature_does_not_expose_removed_provider_flag() {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);

    let provider_flag = sig.named.iter().find(|f| f.long == "provider");
    assert!(
        provider_flag.is_none(),
        "Removed --provider flag must not be exposed"
    );
}

#[test]
fn agent_command_signature_has_model_flag() -> Result<()> {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);

    let model_flag = sig.named.iter().find(|f| f.long == "model");
    assert!(model_flag.is_some(), "Missing --model flag");

    let flag = model_flag.ok_or("should have --model flag")?;
    assert_eq!(flag.short, Some('m'), "Missing -m short flag");
    assert_eq!(
        flag.arg,
        Some(SyntaxShape::String),
        "Wrong type for --model"
    );
    assert!(!flag.desc.is_empty(), "Missing description for --model");
    Ok(())
}

#[test]
fn agent_command_signature_has_api_key_flag() -> Result<()> {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);

    let flag = sig.named.iter().find(|f| f.long == "api-key");
    assert!(flag.is_some(), "Missing --api-key flag");
    let flag = flag.ok_or("should have --api-key flag")?;
    assert_eq!(
        flag.arg,
        Some(SyntaxShape::String),
        "Wrong type for --api-key"
    );
    Ok(())
}

#[test]
fn agent_command_signature_has_base_url_flag() -> Result<()> {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);

    let flag = sig.named.iter().find(|f| f.long == "base-url");
    assert!(flag.is_some(), "Missing --base-url flag");
    let flag = flag.ok_or("should have --base-url flag")?;
    assert_eq!(
        flag.arg,
        Some(SyntaxShape::String),
        "Wrong type for --base-url"
    );
    Ok(())
}

#[test]
fn agent_command_signature_has_temperature_flag() -> Result<()> {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);

    let flag = sig.named.iter().find(|f| f.long == "temperature");
    assert!(flag.is_some(), "Missing --temperature flag");
    let flag = flag.ok_or("should have --temperature flag")?;
    assert_eq!(
        flag.arg,
        Some(SyntaxShape::Number),
        "Wrong type for --temperature"
    );
    Ok(())
}

#[test]
fn agent_command_signature_does_not_expose_removed_max_tokens_flag() {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);

    let flag = sig.named.iter().find(|f| f.long == "max-tokens");
    assert!(
        flag.is_none(),
        "Removed --max-tokens flag must not be exposed"
    );
}

#[test]
fn agent_command_signature_help_text_excludes_removed_flags() {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);
    let rendered = format!("{sig:?}");

    assert!(
        !rendered.contains("long: \"provider\""),
        "signature/help debug output should not contain removed provider long flag"
    );
    assert!(
        !rendered.contains("long: \"max-tokens\""),
        "signature/help debug output should not contain removed max-tokens long flag"
    );
}
#[test]
fn invocation_agent_provider_flag_is_rejected_with_unknown_option_and_help_guidance() -> Result<()>
{
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);
    let parse_errors =
        parse_agent_invocation_with_signature(sig.clone(), "agent --provider openai");

    let (cmd, flag, help) = first_unknown_flag_error(&parse_errors)
        .ok_or("should have unknown flag error for --provider")?;
    assert_eq!(cmd, "agent");
    assert_eq!(flag, "provider");

    let model_flag = sig
        .named
        .iter()
        .find(|f| f.long == "model")
        .ok_or("should have --model flag")?;
    assert!(
        help.contains("--help") && model_flag.desc.contains("provider/model"),
        "when unknown-flag help is generic, canonical guidance must still be present on --model; help={help}, model_desc={} ",
        model_flag.desc
    );
    Ok(())
}

#[test]
fn invocation_agent_max_tokens_flag_is_rejected_with_unknown_option_and_help_guidance() -> Result<()>
{
    let (agent, _temp_dir) = create_test_agent();
    let parse_errors = parse_agent_invocation_with_signature(
        SimplePluginCommand::signature(&agent),
        "agent --max-tokens 4096",
    );

    let (cmd, flag, help) = first_unknown_flag_error(&parse_errors)
        .ok_or("should have unknown flag error for --max-tokens")?;
    assert_eq!(cmd, "agent");
    assert_eq!(flag, "max-tokens");
    assert!(
        help.contains("--max-context-tokens") || help.contains("--max-output-tokens"),
        "unknown --max-tokens guidance should point to explicit token knobs, got: {help}"
    );
    Ok(())
}

#[test]
fn agent_command_signature_has_max_context_tokens_flag() -> Result<()> {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);

    let flag = sig.named.iter().find(|f| f.long == "max-context-tokens");
    assert!(flag.is_some(), "Missing --max-context-tokens flag");
    let flag = flag.ok_or("should have --max-context-tokens flag")?;
    assert_eq!(
        flag.arg,
        Some(SyntaxShape::Int),
        "Wrong type for --max-context-tokens"
    );
    Ok(())
}

#[test]
fn agent_command_signature_has_max_output_tokens_flag() -> Result<()> {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);

    let flag = sig.named.iter().find(|f| f.long == "max-output-tokens");
    assert!(flag.is_some(), "Missing --max-output-tokens flag");
    let flag = flag.ok_or("should have --max-output-tokens flag")?;
    assert_eq!(
        flag.arg,
        Some(SyntaxShape::Int),
        "Wrong type for --max-output-tokens"
    );
    Ok(())
}

#[test]
fn agent_command_signature_has_max_turns_flag() -> Result<()> {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);

    let flag = sig.named.iter().find(|f| f.long == "max-turns");
    assert!(flag.is_some(), "Missing --max-turns flag");
    let flag = flag.ok_or("should have --max-turns flag")?;
    assert_eq!(
        flag.arg,
        Some(SyntaxShape::Int),
        "Wrong type for --max-turns"
    );
    Ok(())
}

#[test]
fn agent_command_signature_has_tools_flag() -> Result<()> {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);

    let flag = sig.named.iter().find(|f| f.long == "tools");
    assert!(flag.is_some(), "Missing --tools flag");
    let flag = flag.ok_or("should have --tools flag")?;
    assert_eq!(
        flag.arg,
        Some(SyntaxShape::Record(vec![].into())),
        "Wrong type for --tools (should be Record)"
    );
    Ok(())
}

#[test]
fn agent_command_signature_has_permissions_flag_as_record() -> Result<()> {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);

    let flag = sig.named.iter().find(|f| f.long == "permissions");
    assert!(flag.is_some(), "Missing --permissions flag");
    let flag = flag.ok_or("should have --permissions flag")?;
    assert_eq!(
        flag.arg,
        Some(SyntaxShape::Record(vec![].into())),
        "--permissions must accept record/object input"
    );
    Ok(())
}

#[test]
fn agent_command_signature_does_not_expose_legacy_permission_flag() {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);

    let legacy = sig.named.iter().find(|f| f.long == "permission");
    assert!(
        legacy.is_none(),
        "Legacy repeated --permission flag must not be exposed"
    );
}

#[test]
fn cli_does_not_expose_unsupported_compaction_modes() {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);
    let rendered = format!("{sig:?}").to_ascii_lowercase();
    // Ensure serde aliases (standalone shorthand names) are not exposed in the CLI.
    // Canonical names like "token_truncate" and "sliding_window" are fine since they
    // appear in the --compaction-strategy description.
    //
    // Strip canonical names before checking for standalone aliases.
    let stripped = rendered
        .replace("sliding_summary", "")
        .replace("sliding_window", "")
        .replace("token_truncate", "");
    assert!(
        !stripped.contains("truncate"),
        "standalone alias 'truncate' should not appear in CLI"
    );
    assert!(
        !stripped.contains("\"sliding\""),
        "standalone alias 'sliding' should not appear in CLI"
    );
    assert!(
        !stripped.contains("summarize"),
        "standalone alias 'summarize' should not appear in CLI"
    );
}

#[test]
fn agent_command_signature_has_quiet_flag() -> Result<()> {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);

    let quiet_flag = sig.named.iter().find(|f| f.long == "quiet");
    assert!(quiet_flag.is_some(), "Missing --quiet flag");

    let flag = quiet_flag.ok_or("should have --quiet flag")?;
    assert_eq!(flag.short, Some('q'), "Missing -q short flag");
    assert_eq!(flag.arg, None, "--quiet should be a switch");
    Ok(())
}

#[test]
fn agent_command_signature_has_log_level_flag() -> Result<()> {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);
    let flag = sig.named.iter().find(|f| f.long == "log-level");
    assert!(flag.is_some(), "Missing --log-level flag");
    let flag = flag.ok_or("should have --log-level flag")?;
    assert_eq!(
        flag.arg,
        Some(SyntaxShape::String),
        "Wrong type for --log-level"
    );
    Ok(())
}

#[test]
fn agent_command_signature_does_not_expose_tui_switch() {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);

    let tui_flag = sig.named.iter().find(|f| f.long == "tui");
    assert!(tui_flag.is_none(), "--tui flag should be removed");
}

#[test]
fn agent_command_signature_updates_verbose_description_for_progressive_levels() -> Result<()> {
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);

    let verbose_flag = sig.named.iter().find(|f| f.long == "verbose");
    assert!(verbose_flag.is_some(), "Missing --verbose flag");
    let desc = &verbose_flag.ok_or("should have --verbose flag")?.desc;
    assert!(
        desc.contains("-v") && desc.contains("-vv") && desc.contains("-vvv"),
        "Verbose description should document progressive levels, got: {desc}"
    );
    Ok(())
}

#[test]
fn agent_command_signature_quiet_and_verbose_help_text_describes_stderr_ux_behavior() -> Result<()>
{
    let (agent, _temp_dir) = create_test_agent();
    let sig = SimplePluginCommand::signature(&agent);

    let quiet = sig
        .named
        .iter()
        .find(|f| f.long == "quiet")
        .ok_or("should have quiet flag")?;
    assert!(
        quiet.desc.contains("Suppress") || quiet.desc.contains("progress"),
        "quiet help text should describe suppression semantics"
    );

    let verbose = sig
        .named
        .iter()
        .find(|f| f.long == "verbose")
        .ok_or("should have verbose flag")?;
    assert!(
        verbose.desc.contains("-v")
            && verbose.desc.contains("-vv")
            && verbose.desc.contains("-vvv"),
        "verbose help text should describe progressive levels"
    );
    Ok(())
}
