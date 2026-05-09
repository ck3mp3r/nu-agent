use super::slash::{
    SLASH_COMMAND_ORDER,
    SlashCommand,
    SlashParseResult,
    filter_inline_slash_suggestions,
    parse_slash_command,
    slash_command_label,
    slash_command_summary,
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
        vec![SlashCommand::Mcp]
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
        ]
    );

    assert_eq!(slash_command_label(SlashCommand::Compact), "/compact");
    assert_eq!(slash_command_label(SlashCommand::Mcp), "/mcp");
    assert_eq!(slash_command_label(SlashCommand::Help), "/help");
    assert_eq!(slash_command_label(SlashCommand::Status), "/status");

    assert!(!slash_command_summary(SlashCommand::Compact).is_empty());
    assert!(!slash_command_summary(SlashCommand::Mcp).is_empty());
    assert!(!slash_command_summary(SlashCommand::Help).is_empty());
    assert!(!slash_command_summary(SlashCommand::Status).is_empty());
}
