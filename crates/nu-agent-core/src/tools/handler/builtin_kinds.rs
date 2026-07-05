use std::str::FromStr;

/// Enum of all built-in tool names.
/// Exactly matches the set used by `is_builtin_tool_name()` in handler/mod.rs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinKind {
    Read,
    Edit,
    Patch,
    Skill,
    SpawnAgent,
    TerminateAgent,
    SendMessage,
    ListAgents,
    Http,
    Grep,
    Glob,
}

impl BuiltinKind {
    /// Canonical name for registry lookup and display.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Edit => "edit",
            Self::Patch => "patch",
            Self::Skill => "skill",
            Self::SpawnAgent => "spawn_agent",
            Self::TerminateAgent => "terminate_agent",
            Self::SendMessage => "send_message",
            Self::ListAgents => "list_agents",
            Self::Http => "http",
            Self::Grep => "grep",
            Self::Glob => "glob",
        }
    }

    /// True for filesystem-mutating tools (edit, patch).
    pub const fn is_fs(&self) -> bool {
        matches!(self, Self::Edit | Self::Patch)
    }
}

impl FromStr for BuiltinKind {
    type Err = ();

    /// Reverse lookup: "read" → Ok(BuiltinKind::Read), "foo" → Err(()).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "read" => Ok(Self::Read),
            "edit" => Ok(Self::Edit),
            "patch" => Ok(Self::Patch),
            "skill" => Ok(Self::Skill),
            "spawn_agent" => Ok(Self::SpawnAgent),
            "terminate_agent" => Ok(Self::TerminateAgent),
            "send_message" => Ok(Self::SendMessage),
            "list_agents" => Ok(Self::ListAgents),
            "http" => Ok(Self::Http),
            "grep" => Ok(Self::Grep),
            "glob" => Ok(Self::Glob),
            _ => Err(()),
        }
    }
}

#[cfg(test)]
#[path = "builtin_kinds_test.rs"]
mod tests;
