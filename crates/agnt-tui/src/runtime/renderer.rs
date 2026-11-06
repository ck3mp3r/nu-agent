use super::*;

use agnt_core::protocol::skills::DiscoverableSkill as ProtocolDiscoverableSkill;
use agnt_runtime::tools::mcp::runtime::McpServerLifecycle;

/// The TUI renderer surface for the command layer: owns the runtime coordinator
/// and the live terminal, and forwards the projections the startup path pushes
/// before the render loop takes the coordinator over.
pub struct TuiRuntimeRenderer {
    pub(crate) coordinator: RuntimeCoordinator,
    live_terminal: Option<LiveTerminalUi>,
}

impl TuiRuntimeRenderer {
    /// Build a renderer with no live terminal surface. Headless and
    /// scripted-input callers drive the coordinator without painting frames.
    pub fn new(columns: u16, rows: u16) -> Self {
        Self {
            coordinator: RuntimeCoordinator::new(columns, rows, None),
            live_terminal: None,
        }
    }

    pub fn new_live(columns: u16, rows: u16) -> Result<Self, String> {
        Ok(Self {
            live_terminal: Some(LiveTerminalUi::new()?),
            ..Self::new(columns, rows)
        })
    }

    pub(crate) fn take_live_terminal(&mut self) -> Option<LiveTerminalUi> {
        self.live_terminal.take()
    }

    pub(crate) fn set_mcp_lifecycle_projection(&mut self, projection: Vec<McpServerLifecycle>) {
        self.coordinator.set_mcp_lifecycle_projection(projection);
    }

    pub(crate) fn set_skills_projection(&mut self, skills: Vec<ProtocolDiscoverableSkill>) {
        self.coordinator.set_skills_projection(skills);
    }

    pub(crate) fn mark_skills_discovery_failed(&mut self) {
        self.coordinator.mark_skills_discovery_failed();
    }

    pub(crate) fn set_repo_branch_caller_cwd(&mut self, caller_cwd: Option<std::path::PathBuf>) {
        self.coordinator.set_repo_branch_caller_cwd(caller_cwd);
    }
}
