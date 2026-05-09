#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SlashCommand {
    Compact,
    Mcp,
    Help,
    Status,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SlashParseResult {
    Command(SlashCommand),
    NotSlash,
    Unknown(String),
}

pub(crate) const SLASH_COMMAND_ORDER: [SlashCommand; 4] = [
    SlashCommand::Compact,
    SlashCommand::Mcp,
    SlashCommand::Help,
    SlashCommand::Status,
];

pub(crate) fn slash_command_label(command: SlashCommand) -> &'static str {
    match command {
        SlashCommand::Compact => "/compact",
        SlashCommand::Mcp => "/mcp",
        SlashCommand::Help => "/help",
        SlashCommand::Status => "/status",
    }
}

pub(crate) fn slash_command_summary(command: SlashCommand) -> &'static str {
    match command {
        SlashCommand::Compact => "Force compaction now",
        SlashCommand::Mcp => "Open MCP servers panel",
        SlashCommand::Help => "Open help panel",
        SlashCommand::Status => "Open status panel",
    }
}

pub(crate) fn filter_inline_slash_suggestions(input: &str) -> Vec<SlashCommand> {
    if !input.starts_with('/') {
        return Vec::new();
    }

    let query = input.trim_start_matches('/').to_ascii_lowercase();
    SLASH_COMMAND_ORDER
        .iter()
        .copied()
        .filter(|command| {
            let candidate = slash_command_label(*command)
                .trim_start_matches('/')
                .to_ascii_lowercase();
            candidate.starts_with(query.as_str())
        })
        .collect()
}

pub(crate) fn parse_slash_command(input: &str) -> SlashParseResult {
    let trimmed = input.trim();
    if !trimmed.starts_with('/') {
        return SlashParseResult::NotSlash;
    }

    let command = trimmed.to_ascii_lowercase();
    let parsed = match command.as_str() {
        "/compact" => Some(SlashCommand::Compact),
        "/mcp" => Some(SlashCommand::Mcp),
        "/help" => Some(SlashCommand::Help),
        "/status" => Some(SlashCommand::Status),
        _ => None,
    };

    if let Some(command) = parsed {
        return SlashParseResult::Command(command);
    }

    SlashParseResult::Unknown(trimmed.to_string())
}
