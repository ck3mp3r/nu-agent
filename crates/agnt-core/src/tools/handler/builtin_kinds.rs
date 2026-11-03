use std::str::FromStr;

/// Enum of all built-in tool names.
/// Exactly matches the set used by `is_builtin_tool_name()` in handler/mod.rs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinKind {
    Read,
    Edit,
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
    AstQuery,
    AstNodes,
    AstRefs,
    AstTree,
}

impl BuiltinKind {
    /// Canonical name for registry lookup and display.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Edit => "edit",
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
            Self::AstQuery => "ast_query",
            Self::AstNodes => "ast_nodes",
            Self::AstRefs => "ast_refs",
            Self::AstTree => "ast_tree",
        }
    }
}

impl FromStr for BuiltinKind {
    type Err = ();

    /// Reverse lookup: "read" → Ok(BuiltinKind::Read), "foo" → Err(()).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "read" => Ok(Self::Read),
            "edit" => Ok(Self::Edit),
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
            "ast_query" => Ok(Self::AstQuery),
            "ast_nodes" => Ok(Self::AstNodes),
            "ast_refs" => Ok(Self::AstRefs),
            "ast_tree" => Ok(Self::AstTree),
            _ => Err(()),
        }
    }
}

/// What kind of tool a call names: a builtin whose line a drawer can tailor, or
/// a tool from an open set (a closure, an MCP server) that only its name
/// identifies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolKind {
    Builtin(BuiltinKind),
    Other,
}

impl ToolKind {
    /// Classify a tool name. The one place the wire's text becomes a variant:
    /// every consumer downstream matches the enum, so adding a builtin fails to
    /// compile wherever a kind is matched exhaustively until it is handled.
    pub fn of_name(name: &str) -> Self {
        match BuiltinKind::from_str(name) {
            Ok(kind) => Self::Builtin(kind),
            Err(()) => Self::Other,
        }
    }
}

#[cfg(test)]
#[path = "../../../test/tools/handler/builtin_kinds.rs"]
mod tests;
