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
    Nu,
    TmuxSession,
    TmuxWindow,
    TmuxPane,
    TmuxLayout,
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
            Self::Nu => "nu",
            Self::TmuxSession => "tmux_session",
            Self::TmuxWindow => "tmux_window",
            Self::TmuxPane => "tmux_pane",
            Self::TmuxLayout => "tmux_layout",
        }
    }

    /// True for filesystem-mutating tools (edit, patch).
    pub const fn is_fs(&self) -> bool {
        matches!(self, Self::Edit | Self::Patch)
    }

    /// True for tmux control tools.
    pub const fn is_tmux(&self) -> bool {
        matches!(
            self,
            Self::TmuxSession | Self::TmuxWindow | Self::TmuxPane | Self::TmuxLayout
        )
    }

    /// True for the Nushell execution tool.
    pub const fn is_nu(&self) -> bool {
        matches!(self, Self::Nu)
    }

    /// True for tools that require elevated privilege (mutating or system-level).
    pub const fn is_privileged(&self) -> bool {
        matches!(
            self,
            Self::Edit
                | Self::Patch
                | Self::TmuxSession
                | Self::TmuxWindow
                | Self::TmuxPane
                | Self::TmuxLayout
                | Self::Nu
        )
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
            "nu" => Ok(Self::Nu),
            "tmux_session" => Ok(Self::TmuxSession),
            "tmux_window" => Ok(Self::TmuxWindow),
            "tmux_pane" => Ok(Self::TmuxPane),
            "tmux_layout" => Ok(Self::TmuxLayout),
            _ => Err(()),
        }
    }
}

#[cfg(test)]
#[path = "builtin_kinds_test.rs"]
mod tests;
