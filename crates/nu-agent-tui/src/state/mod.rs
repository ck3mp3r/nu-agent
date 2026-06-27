mod input;
mod lifecycle;
mod mcp;
mod permissions;
mod picker;
mod prompt_queue;
mod tool_calls;
pub(super) mod transcript;

#[cfg(test)]
mod test;

use crate::markdown::{project_markdown_to_lines, rendered_line_to_plain_text};
use nu_agent_core::protocol::event::{PermissionDecision, PermissionDecisionSubmission};
use nu_agent_core::protocol::slash::{SlashCommand, filter_inline_slash_suggestions};
use nu_agent_core::transcript::ir::{DisplayLine, Role};
use nu_agent_core::transcript::items::{
    ProseMessage, Separator as TranscriptSeparator, Spacer as SpacerItem, SystemMessage,
    ToolInvocation, ToolResult as TranscriptToolResult, TranscriptEntry, annotate_diff_hint,
    parse_tool_text,
};
use ratatui::text::Line;
use std::collections::HashMap;
use std::collections::VecDeque;
use tui_widget_list::ListState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiPhase {
    Idle,
    Busy,
    AbortPending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Insert,
    Normal,
    Visual,
}

impl InputMode {
    pub fn cursor_style(&self) -> crossterm::cursor::SetCursorStyle {
        match self {
            InputMode::Insert => crossterm::cursor::SetCursorStyle::SteadyBar,
            InputMode::Normal | InputMode::Visual => crossterm::cursor::SetCursorStyle::SteadyBlock,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneFocus {
    Transcript,
    Input,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptRole {
    User,
    Assistant,
    System,
    Compaction,
    Tool,
    ToolDisplay,
    Separator,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionStatus {
    InProgress,
    Done,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptLineStatus {
    Prompt(PromptStatus),
    Tool(ToolCallStatus),
    Compaction(CompactionStatus),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionLine {
    pub transcript_line_index: usize,
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
}

impl CommandPaletteAction {
    pub const PALETTE_ACTIONS: &[CommandPaletteAction] = &[
        Self::Help,
        Self::Status,
        Self::Mcps,
        Self::Skills,
        Self::Models,
        Self::Agents,
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
}

pub use nu_agent_core::protocol::picker::{AgentPickerOption, ModelPickerOption};

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
    pub transcript_line_index: usize,
    pub status: PromptStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCallLine {
    pub id: u64,
    pub transcript_line_index: usize,
    pub status: ToolCallStatus,
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionPrompt {
    pub request_id: String,
    pub matched_rule_identity: String,
    pub tool: String,
    pub source: String,
    pub mode: Option<String>,
    pub scope: String,
    pub pattern: String,
    pub target_field: Option<String>,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InputState {
    pub buffer: String,
    pub locked: bool,
    pub cursor: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AbortState {
    pub pending: bool,
    pub confirmation_marker: u64,
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub phase: UiPhase,
    pub input: InputState,
    pub abort: AbortState,
    pub transcript_preview: Vec<TranscriptEntry>,
    pub transcript_list_state: ListState,
    pub status_line: String,
    pub input_mode: InputMode,
    pub pane_focus: PaneFocus,
    pub latest_input_tokens: Option<u64>,
    pub latest_output_tokens: Option<u64>,
    pub latest_total_tokens: Option<u64>,
    pub session_total_tokens: u64,
    context_window_max_tokens: Option<u64>,
    pub quit_requested: bool,
    pub command_palette_open: bool,
    pub command_palette_query: String,
    pub command_palette_selection: usize,
    pub inline_slash_open: bool,
    pub inline_slash_selection: usize,
    pub model_picker_open: bool,
    pub model_picker_query: String,
    pub model_picker_selection: usize,
    pub model_picker_options: Vec<ModelPickerOption>,
    pub agent_picker_open: bool,
    pub agent_picker_query: String,
    pub agent_picker_selection: usize,
    pub agent_picker_options: Vec<AgentPickerOption>,
    pending_agent_picker_launch_requests: usize,
    pending_agent_switch_requests: VecDeque<String>,
    active_agent_identity: Option<String>,
    pub agent_cycle_names: Vec<String>,
    pub info_panel: Option<InfoPanel>,
    pub info_panel_scroll: usize,
    pub mcp_servers: Vec<McpServerState>,
    pub mcp_panel_selection: usize,
    discoverable_skills: Vec<DiscoverableSkill>,
    skills_discovery_failed: bool,
    llm_visible_mcp_tool_count: usize,
    mcp_visible_tool_count_by_server: HashMap<String, usize>,
    mcp_visible_tool_names_by_server: HashMap<String, Vec<String>>,
    mcp_failure_reasons: HashMap<String, String>,
    pending_mcp_toggle_requests: VecDeque<McpToggleRequest>,
    pending_model_switch_requests: VecDeque<String>,
    pending_permission_decisions: VecDeque<PermissionDecisionSubmission>,
    pending_model_picker_launch_requests: usize,
    pub permission_prompt: Option<PermissionPrompt>,
    assistant_projection_cache: HashMap<String, Vec<Line<'static>>>,
    prompt_items: Vec<QueuedPrompt>,
    tool_call_items: Vec<ToolCallLine>,
    compaction_items: Vec<CompactionLine>,
    active_tool_ids_by_key: HashMap<String, VecDeque<u64>>,
    pending_prompt_ids: VecDeque<u64>,
    pending_immediate_submissions: VecDeque<String>,
    active_prompt_id: Option<u64>,
    next_prompt_id: u64,
    next_tool_call_id: u64,
    active_cycle: bool,
    insert_exit_pending_j: bool,
    normal_pending_key: Option<char>,
    inline_slash_commands: Vec<SlashCommand>,
    clipboard_request: Option<String>,
    pub pre_displayed_tool_keys: std::collections::HashSet<String>,
    pub(crate) streaming_message_start: Option<usize>,
    pub(crate) compaction_streaming_start: Option<usize>,
    #[cfg(test)]
    assistant_projection_cache_misses: usize,
}

// Manual PartialEq implementation that skips transcript_list_state comparison
// because tui_widget_list::ListState doesn't implement PartialEq
impl PartialEq for AppState {
    fn eq(&self, other: &Self) -> bool {
        self.phase == other.phase
            && self.input == other.input
            && self.abort == other.abort
            && self.transcript_preview == other.transcript_preview
            // Skip transcript_list_state
            && self.status_line == other.status_line
            && self.input_mode == other.input_mode
            && self.pane_focus == other.pane_focus
            && self.latest_input_tokens == other.latest_input_tokens
            && self.latest_output_tokens == other.latest_output_tokens
            && self.latest_total_tokens == other.latest_total_tokens
            && self.session_total_tokens == other.session_total_tokens
            && self.context_window_max_tokens == other.context_window_max_tokens
            && self.quit_requested == other.quit_requested
            && self.command_palette_open == other.command_palette_open
            && self.command_palette_query == other.command_palette_query
            && self.command_palette_selection == other.command_palette_selection
            && self.inline_slash_open == other.inline_slash_open
            && self.inline_slash_selection == other.inline_slash_selection
            && self.model_picker_open == other.model_picker_open
            && self.model_picker_query == other.model_picker_query
            && self.model_picker_selection == other.model_picker_selection
            && self.model_picker_options == other.model_picker_options
            && self.agent_picker_open == other.agent_picker_open
            && self.agent_picker_query == other.agent_picker_query
            && self.agent_picker_selection == other.agent_picker_selection
            && self.agent_picker_options == other.agent_picker_options
            && self.pending_agent_picker_launch_requests == other.pending_agent_picker_launch_requests
            && self.pending_agent_switch_requests == other.pending_agent_switch_requests
            && self.active_agent_identity == other.active_agent_identity
            && self.agent_cycle_names == other.agent_cycle_names
            && self.info_panel == other.info_panel
            && self.info_panel_scroll == other.info_panel_scroll
            && self.mcp_servers == other.mcp_servers
            && self.mcp_panel_selection == other.mcp_panel_selection
            && self.discoverable_skills == other.discoverable_skills
            && self.skills_discovery_failed == other.skills_discovery_failed
            && self.llm_visible_mcp_tool_count == other.llm_visible_mcp_tool_count
            && self.mcp_visible_tool_count_by_server == other.mcp_visible_tool_count_by_server
            && self.mcp_visible_tool_names_by_server == other.mcp_visible_tool_names_by_server
            && self.mcp_failure_reasons == other.mcp_failure_reasons
            && self.pending_mcp_toggle_requests == other.pending_mcp_toggle_requests
            && self.pending_model_switch_requests == other.pending_model_switch_requests
            && self.pending_permission_decisions == other.pending_permission_decisions
            && self.pending_model_picker_launch_requests == other.pending_model_picker_launch_requests
            && self.permission_prompt == other.permission_prompt
            && self.assistant_projection_cache == other.assistant_projection_cache
            && self.prompt_items == other.prompt_items
            && self.tool_call_items == other.tool_call_items
            && self.compaction_items == other.compaction_items
            && self.active_tool_ids_by_key == other.active_tool_ids_by_key
            && self.pending_prompt_ids == other.pending_prompt_ids
            && self.pending_immediate_submissions == other.pending_immediate_submissions
            && self.active_prompt_id == other.active_prompt_id
            && self.next_prompt_id == other.next_prompt_id
            && self.active_cycle == other.active_cycle
            && self.insert_exit_pending_j == other.insert_exit_pending_j
            && self.normal_pending_key == other.normal_pending_key
            && self.inline_slash_commands == other.inline_slash_commands
            && self.clipboard_request == other.clipboard_request
            && self.pre_displayed_tool_keys == other.pre_displayed_tool_keys
            && self.streaming_message_start == other.streaming_message_start
            && self.compaction_streaming_start == other.compaction_streaming_start
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            phase: UiPhase::Idle,
            input: InputState::default(),
            abort: AbortState::default(),
            transcript_preview: Vec::new(),
            transcript_list_state: ListState::default(),
            status_line: String::new(),
            input_mode: InputMode::Insert,
            pane_focus: PaneFocus::Input,
            latest_input_tokens: None,
            latest_output_tokens: None,
            latest_total_tokens: None,
            session_total_tokens: 0,
            context_window_max_tokens: None,
            quit_requested: false,
            command_palette_open: false,
            command_palette_query: String::new(),
            command_palette_selection: 0,
            inline_slash_open: false,
            inline_slash_selection: 0,
            model_picker_open: false,
            model_picker_query: String::new(),
            model_picker_selection: 0,
            model_picker_options: Vec::new(),
            agent_picker_open: false,
            agent_picker_query: String::new(),
            agent_picker_selection: 0,
            agent_picker_options: Vec::new(),
            pending_agent_picker_launch_requests: 0,
            pending_agent_switch_requests: VecDeque::new(),
            active_agent_identity: None,
            agent_cycle_names: Vec::new(),
            info_panel: None,
            info_panel_scroll: 0,
            mcp_servers: Vec::new(),
            mcp_panel_selection: 0,
            discoverable_skills: Vec::new(),
            skills_discovery_failed: false,
            llm_visible_mcp_tool_count: 0,
            mcp_visible_tool_count_by_server: HashMap::new(),
            mcp_visible_tool_names_by_server: HashMap::new(),
            mcp_failure_reasons: HashMap::new(),
            pending_mcp_toggle_requests: VecDeque::new(),
            pending_model_switch_requests: VecDeque::new(),
            pending_permission_decisions: VecDeque::new(),
            pending_model_picker_launch_requests: 0,
            permission_prompt: None,
            assistant_projection_cache: HashMap::new(),
            prompt_items: Vec::new(),
            tool_call_items: Vec::new(),
            compaction_items: Vec::new(),
            active_tool_ids_by_key: HashMap::new(),
            pending_prompt_ids: VecDeque::new(),
            pending_immediate_submissions: VecDeque::new(),
            active_prompt_id: None,
            next_prompt_id: 1,
            next_tool_call_id: 1,
            active_cycle: false,
            insert_exit_pending_j: false,
            normal_pending_key: None,
            inline_slash_commands: Vec::new(),
            clipboard_request: None,
            pre_displayed_tool_keys: std::collections::HashSet::new(),
            streaming_message_start: None,
            compaction_streaming_start: None,
            #[cfg(test)]
            assistant_projection_cache_misses: 0,
        }
    }
}
