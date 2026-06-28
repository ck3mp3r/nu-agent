#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlashCommand {
    Compact,
    Mcp,
    Help,
    Status,
    Models,
    Agent,
    New,
}

impl SlashCommand {
    pub fn label(&self) -> &'static str {
        match self {
            SlashCommand::Compact => "/compact",
            SlashCommand::Mcp => "/mcp",
            SlashCommand::Help => "/help",
            SlashCommand::Status => "/status",
            SlashCommand::Models => "/models",
            SlashCommand::Agent => "/agent",
            SlashCommand::New => "/new",
        }
    }

    pub fn summary(&self) -> &'static str {
        match self {
            SlashCommand::Compact => "Force compaction now",
            SlashCommand::Mcp => "Open MCP servers panel",
            SlashCommand::Help => "Open help panel",
            SlashCommand::Status => "Open status panel",
            SlashCommand::Models => "Open model picker",
            SlashCommand::Agent => "Switch agent persona",
            SlashCommand::New => "Start a new session",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlashParseResult {
    Command(SlashCommand),
    NotSlash,
    Unknown(String),
}

pub const SLASH_COMMAND_ORDER: [SlashCommand; 7] = [
    SlashCommand::Compact,
    SlashCommand::Mcp,
    SlashCommand::Help,
    SlashCommand::Status,
    SlashCommand::Models,
    SlashCommand::Agent,
    SlashCommand::New,
];

pub fn filter_inline_slash_suggestions(input: &str) -> Vec<SlashCommand> {
    if !input.starts_with('/') {
        return Vec::new();
    }

    let query = input.trim_start_matches('/').to_ascii_lowercase();
    SLASH_COMMAND_ORDER
        .iter()
        .copied()
        .filter(|command| {
            let candidate = command.label().trim_start_matches('/').to_ascii_lowercase();
            candidate.starts_with(query.as_str())
        })
        .collect()
}

pub fn parse_slash_command(input: &str) -> SlashParseResult {
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
        "/models" => Some(SlashCommand::Models),
        "/agent" => Some(SlashCommand::Agent),
        "/new" => Some(SlashCommand::New),
        _ => None,
    };

    if let Some(command) = parsed {
        return SlashParseResult::Command(command);
    }

    SlashParseResult::Unknown(trimmed.to_string())
}
