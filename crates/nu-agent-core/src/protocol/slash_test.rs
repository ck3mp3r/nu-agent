use super::slash::{
    SLASH_COMMAND_ORDER, SlashCommand, SlashParseResult, filter_inline_slash_suggestions,
    parse_slash_command,
};

#[test]
fn parse_slash_command_compact_mcp_help_status_exact() {
    assert_eq!(
        parse_slash_command("   /compact   "),
        SlashParseResult::Command(SlashCommand::Compact)
    );
    assert_eq!(
        parse_slash_command(" /mcp "),
        SlashParseResult::Command(SlashCommand::Mcp)
    );
    assert_eq!(
        parse_slash_command(" /help "),
        SlashParseResult::Command(SlashCommand::Help)
    );
    assert_eq!(
        parse_slash_command(" /status "),
        SlashParseResult::Command(SlashCommand::Status)
    );
}

#[test]
fn parse_slash_command_models_exact() {
    assert_eq!(
        parse_slash_command(" /models "),
        SlashParseResult::Command(SlashCommand::Models)
    );
}

#[test]
fn parse_slash_command_non_slash_returns_not_slash() {
    assert_eq!(
        parse_slash_command("hello world"),
        SlashParseResult::NotSlash
    );
}

#[test]
fn parse_slash_command_unknown_returns_unknown() {
    assert_eq!(
        parse_slash_command("/compact now"),
        SlashParseResult::Unknown("/compact now".to_string())
    );
}

#[test]
fn inline_slash_filter_is_prefix_based_and_deterministic() {
    assert_eq!(
        filter_inline_slash_suggestions("/"),
        vec![
            SlashCommand::Compact,
            SlashCommand::Mcp,
            SlashCommand::Help,
            SlashCommand::Status,
            SlashCommand::Models,
            SlashCommand::Agent,
            SlashCommand::New,
        ]
    );
    assert_eq!(
        filter_inline_slash_suggestions("/c"),
        vec![SlashCommand::Compact]
    );
    assert_eq!(
        filter_inline_slash_suggestions("/co"),
        vec![SlashCommand::Compact]
    );
    assert_eq!(
        filter_inline_slash_suggestions("/m"),
        vec![SlashCommand::Mcp, SlashCommand::Models]
    );
    assert!(filter_inline_slash_suggestions("/x").is_empty());
    assert!(filter_inline_slash_suggestions("hello").is_empty());
}

#[test]
fn slash_command_catalog_exports_expected_labels_and_order() {
    assert_eq!(
        SLASH_COMMAND_ORDER,
        [
            SlashCommand::Compact,
            SlashCommand::Mcp,
            SlashCommand::Help,
            SlashCommand::Status,
            SlashCommand::Models,
            SlashCommand::Agent,
            SlashCommand::New,
        ]
    );

    assert_eq!(SlashCommand::Compact.label(), "/compact");
    assert_eq!(SlashCommand::Mcp.label(), "/mcp");
    assert_eq!(SlashCommand::Help.label(), "/help");
    assert_eq!(SlashCommand::Status.label(), "/status");
    assert_eq!(SlashCommand::Models.label(), "/models");
    assert_eq!(SlashCommand::Agent.label(), "/agent");
    assert_eq!(SlashCommand::New.label(), "/new");

    assert!(!SlashCommand::Compact.summary().is_empty());
    assert!(!SlashCommand::Mcp.summary().is_empty());
    assert!(!SlashCommand::Help.summary().is_empty());
    assert!(!SlashCommand::Status.summary().is_empty());
    assert!(!SlashCommand::Models.summary().is_empty());
    assert!(!SlashCommand::Agent.summary().is_empty());
    assert!(!SlashCommand::New.summary().is_empty());
}

#[test]
fn parse_slash_command_agent_exact() {
    assert_eq!(
        parse_slash_command("/agent"),
        SlashParseResult::Command(SlashCommand::Agent)
    );
}

#[test]
fn parse_slash_command_agents_does_not_match() {
    assert_eq!(
        parse_slash_command("/agents"),
        SlashParseResult::Unknown("/agents".to_string())
    );
}

#[test]
fn slash_command_label_agent_returns_slash_agent() {
    assert_eq!(SlashCommand::Agent.label(), "/agent");
}

#[test]
fn slash_command_summary_agent_returns_switch_agent_persona() {
    assert_eq!(SlashCommand::Agent.summary(), "Switch agent persona");
}
