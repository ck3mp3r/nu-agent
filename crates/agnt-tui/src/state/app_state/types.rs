//! The state value's own vocabulary: phases, statuses, queued rows, the abort
//! arm, and the palette/panel enumerations the surface derives from.

use std::time::{Duration, Instant};

use crate::state::status::NEUTRAL_MESSAGE_TTL;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiPhase {
    Idle,
    Busy,
    AbortPending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptRole {
    User,
    Assistant,
    System,
    Compaction,
    Tool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptStatus {
    Queued,
    InProgress,
    Done,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCallStatus {
    InProgress,
    Done,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionStatus {
    InProgress,
    Done,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionLine {
    pub source: String,
    pub status: CompactionStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandPaletteAction {
    Compact,
    Help,
    Status,
    Mcps,
    Skills,
    Models,
    Agents,
    Sessions,
    Theme,
}

impl CommandPaletteAction {
    pub const PALETTE_ACTIONS: &[CommandPaletteAction] = &[
        Self::Help,
        Self::Status,
        Self::Mcps,
        Self::Skills,
        Self::Models,
        Self::Agents,
        Self::Sessions,
        Self::Theme,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Self::Compact => "/compact",
            Self::Help => "Help",
            Self::Status => "Status",
            Self::Mcps => "MCPs",
            Self::Skills => "Skills",
            Self::Models => "Models",
            Self::Agents => "Agents",
            Self::Sessions => "Sessions",
            Self::Theme => "Theme",
        }
    }

    pub fn summary(&self) -> &'static str {
        match self {
            Self::Compact => "Run /compact now",
            Self::Help => "View key help",
            Self::Status => "View runtime status",
            Self::Mcps => "Manage MCP servers",
            Self::Skills => "List available skills",
            Self::Models => "Open model picker",
            Self::Agents => "Open agent picker",
            Self::Sessions => "Switch to an existing session",
            Self::Theme => "Open theme picker",
        }
    }

    pub fn info_panel(&self) -> Option<InfoPanel> {
        match self {
            Self::Compact => None,
            Self::Help => Some(InfoPanel::Help),
            Self::Status => Some(InfoPanel::Status),
            Self::Mcps => Some(InfoPanel::Mcps),
            Self::Skills => Some(InfoPanel::Skills),
            Self::Models => None,
            Self::Agents => None,
            Self::Sessions => None,
            Self::Theme => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InfoPanel {
    Help,
    Status,
    Mcps,
    Skills,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoverableSkill {
    pub source_priority: u8,
    pub source: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerState {
    pub name: String,
    pub state: McpServerUsabilityState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpToggleRequest {
    pub server_name: String,
    pub enable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpServerUsabilityState {
    Enabled,
    Disabled,
    Failed,
}

impl McpServerUsabilityState {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
            Self::Failed => "failed",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Self::Enabled => "🟢",
            Self::Disabled => "⚪",
            Self::Failed => "🔴",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedPrompt {
    pub id: u64,
    pub prompt_text: String,
    pub status: PromptStatus,
    /// The footer facts the submit boundary stamped: carried from the enqueue
    /// to the transcript row, verbatim.
    pub facts: PromptFacts,
}

/// The footer facts one submission carries from its boundary to its row.
///
/// The submit instant and the identities active when the prompt was accepted.
/// Every field is what the boundary knew: an unknown fact stays absent, and the
/// row it reaches carries exactly this.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PromptFacts {
    pub millis: Option<i64>,
    pub model: Option<String>,
    pub agent: Option<String>,
}

impl PromptFacts {
    /// Fold `later` into `self`: the first `Some` per field wins, in queue order.
    pub fn merge_first_some(&mut self, later: &PromptFacts) {
        if self.millis.is_none() {
            self.millis = later.millis;
        }
        if self.model.is_none() {
            self.model.clone_from(&later.model);
        }
        if self.agent.is_none() {
            self.agent.clone_from(&later.agent);
        }
    }
}

/// One prompt taken for immediate execution: its text and the facts its submit
/// boundary stamped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImmediateSubmission {
    pub text: String,
    pub facts: PromptFacts,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCallLine {
    pub id: u64,
    pub status: ToolCallStatus,
    key: agnt_runtime::protocol::events::ToolCallId,
    pub block_index: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbortState {
    pub pending: bool,
    pub confirmation_marker: u64,
    /// An armed abort lost its turn to the settle before the confirming press
    /// arrived. The notice the user is owed is spent by the first press that
    /// lands on an idle surface.
    pub settle_notice: bool,
    /// When the arm was stamped, if it is armed.
    pub armed_at: Option<Instant>,
    /// How long an armed abort stays executable. The arm lives exactly as long
    /// as its affordance message, so this is the Neutral message TTL; tests set
    /// a shorter window on the field.
    pub arm_window: Duration,
}

impl Default for AbortState {
    fn default() -> Self {
        Self {
            pending: false,
            confirmation_marker: 0,
            settle_notice: false,
            armed_at: None,
            arm_window: NEUTRAL_MESSAGE_TTL,
        }
    }
}

impl AbortState {
    /// True only if the arm is stamped AND its message lifetime is not over.
    /// The press that finds an expired arm disarms it and re-arms instead of
    /// executing.
    pub fn arm_live(&self) -> bool {
        match self.armed_at {
            Some(armed_at) => armed_at.elapsed() < self.arm_window,
            None => false,
        }
    }
}

impl ToolCallLine {
    /// Build a row from the call it represents: the key is the live call's own
    /// id, adopted at construction and never replaced.
    pub(crate) fn new(
        id: u64,
        status: ToolCallStatus,
        key: agnt_runtime::protocol::events::ToolCallId,
        block_index: Option<usize>,
    ) -> Self {
        Self {
            id,
            status,
            key,
            block_index,
        }
    }

    /// The call this row represents.
    pub(crate) fn key(&self) -> &agnt_runtime::protocol::events::ToolCallId {
        &self.key
    }
}
