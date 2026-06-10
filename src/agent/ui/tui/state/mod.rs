mod prompt_queue;
mod tool_calls;

#[cfg(test)]
mod test;

use crate::agent::protocol::event::{PermissionDecision, PermissionDecisionSubmission};
use crate::agent::protocol::slash::{SlashCommand, filter_inline_slash_suggestions};
use crate::agent::ui::transcript::ir::{ContentLine, DisplayLine, Role, Span, StyleHint};
use crate::agent::ui::transcript::items::{
    AssistantChunk, Separator as TranscriptSeparator, Spacer as SpacerItem, SystemMessage,
    ToolInvocation, ToolResult as TranscriptToolResult, TranscriptEntry, UserMessage,
    annotate_diff_hint, parse_tool_text,
};
use crate::agent::ui::tui::markdown::{project_markdown_to_lines, rendered_line_to_plain_text};
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelPickerOption {
    pub provider: String,
    pub model: String,
    pub identity: String,
    pub display: String,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentPickerOption {
    pub name: String,
    pub description: Option<String>,
    pub display: String,
    pub active: bool,
    pub builtin: bool,
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

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_active_cycle(&self) -> bool {
        self.active_cycle
    }

    #[cfg(test)]
    pub fn prompt_items(&self) -> &[QueuedPrompt] {
        &self.prompt_items
    }

    #[cfg(test)]
    pub fn active_prompt_id(&self) -> Option<u64> {
        self.active_prompt_id
    }

    #[cfg(test)]
    pub fn pending_prompt_ids(&self) -> &VecDeque<u64> {
        &self.pending_prompt_ids
    }

    pub fn pending_prompt_count(&self) -> usize {
        self.pending_prompt_ids.len()
    }

    pub fn command_palette_actions(&self) -> Vec<CommandPaletteAction> {
        let canonical = [
            (CommandPaletteAction::Help, "Help"),
            (CommandPaletteAction::Status, "Status"),
            (CommandPaletteAction::Mcps, "MCPs"),
            (CommandPaletteAction::Skills, "Skills"),
            (CommandPaletteAction::Models, "Models"),
            (CommandPaletteAction::Agents, "Agents"),
        ];

        if self.command_palette_query.is_empty() {
            return canonical.iter().map(|(action, _)| *action).collect();
        }

        let query = self.command_palette_query.to_ascii_lowercase();
        canonical
            .iter()
            .filter_map(|(action, label)| {
                fuzzy_matches(&query, &label.to_ascii_lowercase()).then_some(*action)
            })
            .collect()
    }

    pub fn command_palette_selected_action(&self) -> Option<CommandPaletteAction> {
        self.command_palette_actions()
            .get(self.command_palette_selection)
            .copied()
    }

    pub fn open_command_palette(&mut self) {
        self.info_panel = None;
        self.command_palette_open = true;
        self.command_palette_query.clear();
        self.command_palette_selection = 0;
    }

    pub fn close_command_palette(&mut self) {
        self.command_palette_open = false;
    }

    pub(crate) fn inline_slash_suggestions(&self) -> &[SlashCommand] {
        &self.inline_slash_commands
    }

    pub(crate) fn inline_slash_selected_command(&self) -> Option<SlashCommand> {
        self.inline_slash_commands
            .get(self.inline_slash_selection)
            .copied()
    }

    pub(crate) fn close_inline_slash_suggestions(&mut self) {
        self.inline_slash_open = false;
        self.inline_slash_selection = 0;
        self.inline_slash_commands.clear();
    }

    pub(crate) fn inline_slash_move_up(&mut self) {
        let len = self.inline_slash_commands.len();
        if len == 0 {
            self.inline_slash_selection = 0;
            return;
        }

        self.inline_slash_selection = if self.inline_slash_selection == 0 {
            len.saturating_sub(1)
        } else {
            self.inline_slash_selection.saturating_sub(1)
        };
    }

    pub(crate) fn inline_slash_move_down(&mut self) {
        let len = self.inline_slash_commands.len();
        if len == 0 {
            self.inline_slash_selection = 0;
            return;
        }

        self.inline_slash_selection = (self.inline_slash_selection + 1) % len;
    }

    pub fn open_info_panel(&mut self, panel: InfoPanel) {
        self.command_palette_open = false;
        self.model_picker_open = false;
        self.info_panel = Some(panel);
        self.info_panel_scroll = 0;
    }

    pub fn close_info_panel(&mut self) {
        self.info_panel = None;
        self.info_panel_scroll = 0;
    }

    pub fn set_mcp_servers(&mut self, servers: Vec<McpServerState>) {
        self.mcp_servers = servers;
        self.mcp_visible_tool_count_by_server
            .retain(|name, _| self.mcp_servers.iter().any(|server| server.name == *name));
        self.mcp_visible_tool_names_by_server
            .retain(|name, _| self.mcp_servers.iter().any(|server| server.name == *name));
        self.mcp_failure_reasons.retain(|name, _| {
            self.mcp_servers.iter().any(|server| {
                server.name == *name && server.state == McpServerUsabilityState::Failed
            })
        });
        if self.mcp_servers.is_empty() {
            self.mcp_panel_selection = 0;
        } else if self.mcp_panel_selection >= self.mcp_servers.len() {
            self.mcp_panel_selection = self.mcp_servers.len().saturating_sub(1);
        }
    }

    pub fn set_llm_visible_mcp_tool_count(&mut self, count: usize) {
        self.llm_visible_mcp_tool_count = count;
    }

    pub fn set_mcp_visible_tool_count_by_server_name(&mut self, server_name: &str, count: usize) {
        self.mcp_visible_tool_count_by_server
            .insert(server_name.to_string(), count);
    }

    pub fn mcp_visible_tool_count_for_server_name(&self, server_name: &str) -> usize {
        self.mcp_visible_tool_count_by_server
            .get(server_name)
            .copied()
            .unwrap_or(0)
    }

    pub fn set_mcp_visible_tool_names_by_server_name(
        &mut self,
        server_name: &str,
        mut names: Vec<String>,
    ) {
        names.sort();
        names.dedup();
        self.mcp_visible_tool_names_by_server
            .insert(server_name.to_string(), names);
    }

    pub fn mcp_visible_tool_names_for_server_name(&self, server_name: &str) -> Vec<String> {
        self.mcp_visible_tool_names_by_server
            .get(server_name)
            .cloned()
            .unwrap_or_default()
    }

    pub fn set_discoverable_skills(&mut self, mut skills: Vec<DiscoverableSkill>) {
        skills.sort_by(|left, right| {
            left.source_priority
                .cmp(&right.source_priority)
                .then_with(|| {
                    left.name
                        .to_ascii_lowercase()
                        .cmp(&right.name.to_ascii_lowercase())
                })
                .then_with(|| {
                    left.source
                        .to_ascii_lowercase()
                        .cmp(&right.source.to_ascii_lowercase())
                })
        });
        self.discoverable_skills = skills;
        self.skills_discovery_failed = false;
    }

    pub fn mark_skills_discovery_failed(&mut self) {
        self.discoverable_skills.clear();
        self.skills_discovery_failed = true;
    }

    pub fn set_model_picker_options(&mut self, mut options: Vec<ModelPickerOption>) {
        options.sort_by(|left, right| {
            left.provider
                .to_ascii_lowercase()
                .cmp(&right.provider.to_ascii_lowercase())
                .then_with(|| {
                    left.model
                        .to_ascii_lowercase()
                        .cmp(&right.model.to_ascii_lowercase())
                })
                .then_with(|| {
                    left.identity
                        .to_ascii_lowercase()
                        .cmp(&right.identity.to_ascii_lowercase())
                })
        });
        self.model_picker_options = options;
        self.ensure_invariants();
    }

    pub fn open_model_picker(&mut self) {
        self.command_palette_open = false;
        self.info_panel = None;
        self.model_picker_open = true;
        self.model_picker_query.clear();
        self.model_picker_selection = 0;
        self.ensure_invariants();
    }

    pub fn close_model_picker(&mut self) {
        self.model_picker_open = false;
        self.model_picker_query.clear();
        self.model_picker_selection = 0;
        self.ensure_invariants();
    }

    pub fn model_picker_close_on_escape(&mut self) {
        self.close_model_picker();
    }

    pub fn model_picker_move_up(&mut self) {
        let len = self.model_picker_filtered_options().len();
        if len == 0 {
            self.model_picker_selection = 0;
            return;
        }

        self.model_picker_selection = if self.model_picker_selection == 0 {
            len.saturating_sub(1)
        } else {
            self.model_picker_selection.saturating_sub(1)
        };
    }

    pub fn model_picker_move_down(&mut self) {
        let len = self.model_picker_filtered_options().len();
        if len == 0 {
            self.model_picker_selection = 0;
            return;
        }

        self.model_picker_selection = (self.model_picker_selection + 1) % len;
    }

    pub fn append_model_picker_query_char(&mut self, ch: char) {
        self.model_picker_query.push(ch);
        self.model_picker_selection = 0;
        self.ensure_invariants();
    }

    pub fn backspace_model_picker_query_char(&mut self) {
        self.model_picker_query.pop();
        self.model_picker_selection = 0;
        self.ensure_invariants();
    }

    pub fn model_picker_filtered_options(&self) -> Vec<ModelPickerOption> {
        if self.model_picker_query.is_empty() {
            return self.model_picker_options.clone();
        }

        let query = self.model_picker_query.to_ascii_lowercase();
        self.model_picker_options
            .iter()
            .filter(|option| {
                option
                    .identity
                    .to_ascii_lowercase()
                    .contains(query.as_str())
                    || option.display.to_ascii_lowercase().contains(query.as_str())
            })
            .cloned()
            .collect()
    }

    pub fn selected_model_picker_option(&self) -> Option<ModelPickerOption> {
        self.model_picker_filtered_options()
            .get(self.model_picker_selection)
            .cloned()
    }

    pub fn queue_selected_model_switch_request(&mut self) -> bool {
        let Some(selected) = self.selected_model_picker_option() else {
            return false;
        };
        self.pending_model_switch_requests
            .push_back(selected.identity.clone());
        true
    }

    pub fn take_next_model_switch_request(&mut self) -> Option<String> {
        self.pending_model_switch_requests.pop_front()
    }

    pub fn open_permission_prompt(&mut self, prompt: PermissionPrompt) {
        self.permission_prompt = Some(prompt);
        self.status_line = "Permission required".to_string();
        self.scroll_transcript_to_bottom();
        self.ensure_invariants();
    }

    pub fn focus_transcript_pane(&mut self) {
        self.pane_focus = PaneFocus::Transcript;
    }

    pub fn latest_in_progress_tool_transcript_line_for_tool(
        &self,
        tool_name: &str,
    ) -> Option<usize> {
        // Extract base tool name from display format "tool_name(args...)" or "tool_name"
        let base_tool_name = tool_name.split('(').next().unwrap_or(tool_name);

        self.tool_call_items
            .iter()
            .rev()
            .find(|item| {
                item.status == ToolCallStatus::InProgress
                    && item
                        .key
                        .split_once('\n')
                        .map(|(name, _)| name == base_tool_name)
                        .unwrap_or(false)
            })
            .map(|item| item.transcript_line_index)
    }

    pub fn latest_in_progress_tool_key_for_tool(&self, tool_name: &str) -> Option<String> {
        let base_tool_name = tool_name.split('(').next().unwrap_or(tool_name);

        self.tool_call_items
            .iter()
            .rev()
            .find(|item| {
                item.status == ToolCallStatus::InProgress
                    && item
                        .key
                        .split_once('\n')
                        .map(|(name, _)| name == base_tool_name)
                        .unwrap_or(false)
            })
            .map(|item| item.key.clone())
    }

    pub fn has_permission_prompt(&self) -> bool {
        self.permission_prompt.is_some()
    }

    pub fn submit_permission_decision(&mut self, decision: PermissionDecision) -> bool {
        let Some(prompt) = self.permission_prompt.as_ref() else {
            return false;
        };
        self.pending_permission_decisions
            .push_back(PermissionDecisionSubmission {
                request_id: prompt.request_id.clone(),
                decision,
                matched_rule_identity: prompt.matched_rule_identity.clone(),
            });
        self.permission_prompt = None;
        self.ensure_invariants();
        true
    }

    pub fn take_next_permission_decision_submission(
        &mut self,
    ) -> Option<PermissionDecisionSubmission> {
        self.pending_permission_decisions.pop_front()
    }

    pub fn queue_model_picker_launch_request(&mut self) {
        self.pending_model_picker_launch_requests =
            self.pending_model_picker_launch_requests.saturating_add(1);
        self.input.buffer.clear();
        self.input.cursor = 0;
        self.abort.pending = false;
        self.ensure_invariants();
    }

    pub fn take_next_model_picker_launch_request(&mut self) -> bool {
        if self.pending_model_picker_launch_requests == 0 {
            return false;
        }
        self.pending_model_picker_launch_requests =
            self.pending_model_picker_launch_requests.saturating_sub(1);
        true
    }

    pub fn set_agent_picker_options(&mut self, options: Vec<AgentPickerOption>) {
        let mut options = options;
        options.sort_by(|a, b| a.name.cmp(&b.name));
        self.agent_picker_options = options;
        self.ensure_invariants();
    }

    pub fn open_agent_picker(&mut self) {
        self.close_command_palette();
        self.close_info_panel();
        self.agent_picker_open = true;
        self.agent_picker_query.clear();
        self.agent_picker_selection = 0;
        self.ensure_invariants();
    }

    pub fn close_agent_picker(&mut self) {
        self.agent_picker_open = false;
        self.agent_picker_query.clear();
        self.agent_picker_selection = 0;
        self.ensure_invariants();
    }

    pub fn agent_picker_close_on_escape(&mut self) {
        self.close_agent_picker();
    }

    pub fn agent_picker_move_up(&mut self) {
        let count = self.agent_picker_filtered_options().len();
        if count == 0 {
            return;
        }
        if self.agent_picker_selection == 0 {
            self.agent_picker_selection = count - 1;
        } else {
            self.agent_picker_selection -= 1;
        }
    }

    pub fn agent_picker_move_down(&mut self) {
        let count = self.agent_picker_filtered_options().len();
        if count == 0 {
            return;
        }
        self.agent_picker_selection = (self.agent_picker_selection + 1) % count;
    }

    pub fn append_agent_picker_query_char(&mut self, ch: char) {
        self.agent_picker_query.push(ch);
        self.agent_picker_selection = 0;
        self.ensure_invariants();
    }

    pub fn backspace_agent_picker_query_char(&mut self) {
        self.agent_picker_query.pop();
        self.agent_picker_selection = 0;
        self.ensure_invariants();
    }

    pub fn agent_picker_filtered_options(&self) -> Vec<AgentPickerOption> {
        if self.agent_picker_query.is_empty() {
            return self.agent_picker_options.clone();
        }
        let query = self.agent_picker_query.to_lowercase();
        self.agent_picker_options
            .iter()
            .filter(|o| {
                o.name.to_lowercase().contains(&query) || o.display.to_lowercase().contains(&query)
            })
            .cloned()
            .collect()
    }

    pub fn selected_agent_picker_option(&self) -> Option<AgentPickerOption> {
        let filtered = self.agent_picker_filtered_options();
        filtered.get(self.agent_picker_selection).cloned()
    }

    pub fn queue_selected_agent_switch_request(&mut self) -> bool {
        if let Some(option) = self.selected_agent_picker_option() {
            self.pending_agent_switch_requests.push_back(option.name);
            true
        } else {
            false
        }
    }

    pub fn take_next_agent_switch_request(&mut self) -> Option<String> {
        self.pending_agent_switch_requests.pop_front()
    }

    pub fn queue_agent_picker_launch_request(&mut self) {
        self.pending_agent_picker_launch_requests =
            self.pending_agent_picker_launch_requests.saturating_add(1);
        self.input.buffer.clear();
        self.input.cursor = 0;
        self.abort.pending = false;
        self.ensure_invariants();
    }

    pub fn take_next_agent_picker_launch_request(&mut self) -> bool {
        if self.pending_agent_picker_launch_requests > 0 {
            self.pending_agent_picker_launch_requests -= 1;
            true
        } else {
            false
        }
    }

    pub fn set_active_agent_identity(&mut self, name: &str) {
        self.active_agent_identity = Some(name.to_string());
        for option in &mut self.agent_picker_options {
            option.active = option.name == name;
        }
    }

    pub fn active_agent_identity(&self) -> Option<&str> {
        self.active_agent_identity.as_deref()
    }

    pub fn has_agents_to_cycle(&self) -> bool {
        self.agent_cycle_names.len() >= 2
    }

    pub fn next_agent_cycle_name(&self) -> Option<String> {
        if !self.has_agents_to_cycle() {
            return None;
        }
        let current = self.active_agent_identity.as_deref().unwrap_or("");
        let current_idx = self
            .agent_cycle_names
            .iter()
            .position(|n| n == current)
            .unwrap_or(0);
        let next_idx = (current_idx + 1) % self.agent_cycle_names.len();
        Some(self.agent_cycle_names[next_idx].clone())
    }

    pub fn queue_cycle_agent_request(&mut self) {
        if let Some(next_name) = self.next_agent_cycle_name() {
            self.pending_agent_switch_requests.push_back(next_name);
        }
    }

    pub fn discoverable_skills(&self) -> &[DiscoverableSkill] {
        &self.discoverable_skills
    }

    pub fn skills_discovery_failed(&self) -> bool {
        self.skills_discovery_failed
    }

    pub fn llm_visible_mcp_tool_count(&self) -> usize {
        self.llm_visible_mcp_tool_count
    }

    pub fn mcp_panel_move_up(&mut self) {
        let len = self.mcp_servers.len();
        if len == 0 {
            self.mcp_panel_selection = 0;
            return;
        }

        self.mcp_panel_selection = if self.mcp_panel_selection == 0 {
            len.saturating_sub(1)
        } else {
            self.mcp_panel_selection.saturating_sub(1)
        };
    }

    pub fn mcp_panel_move_down(&mut self) {
        let len = self.mcp_servers.len();
        if len == 0 {
            self.mcp_panel_selection = 0;
            return;
        }

        self.mcp_panel_selection = (self.mcp_panel_selection + 1) % len;
    }

    pub fn selected_mcp_server_name(&self) -> Option<&str> {
        self.mcp_servers
            .get(self.mcp_panel_selection)
            .map(|server| server.name.as_str())
    }

    pub fn selected_mcp_server_state(&self) -> Option<McpServerUsabilityState> {
        self.mcp_servers
            .get(self.mcp_panel_selection)
            .map(|server| server.state)
    }

    pub fn set_mcp_server_state_by_name(
        &mut self,
        name: &str,
        state: McpServerUsabilityState,
    ) -> bool {
        self.set_mcp_server_state_by_name_with_reason(name, state, None)
    }

    pub fn set_mcp_server_state_by_name_with_reason(
        &mut self,
        name: &str,
        state: McpServerUsabilityState,
        reason: Option<String>,
    ) -> bool {
        if let Some(server) = self
            .mcp_servers
            .iter_mut()
            .find(|server| server.name == name)
        {
            server.state = state;

            match state {
                McpServerUsabilityState::Failed => {
                    if let Some(reason) = reason {
                        let trimmed = reason.trim();
                        if !trimmed.is_empty() {
                            self.mcp_failure_reasons
                                .insert(name.to_string(), trimmed.to_string());
                        }
                    }
                }
                McpServerUsabilityState::Enabled | McpServerUsabilityState::Disabled => {
                    self.mcp_failure_reasons.remove(name);
                }
            }

            return true;
        }
        false
    }

    pub fn failed_mcp_servers_with_reasons(&self) -> Vec<(&str, Option<&str>)> {
        self.mcp_servers
            .iter()
            .filter(|server| server.state == McpServerUsabilityState::Failed)
            .map(|server| {
                (
                    server.name.as_str(),
                    self.mcp_failure_reasons
                        .get(server.name.as_str())
                        .map(String::as_str),
                )
            })
            .collect()
    }

    pub fn queue_selected_mcp_toggle_request(&mut self) -> bool {
        let Some(server) = self.mcp_servers.get_mut(self.mcp_panel_selection) else {
            return false;
        };

        let request = match server.state {
            McpServerUsabilityState::Enabled => {
                server.state = McpServerUsabilityState::Disabled;
                McpToggleRequest {
                    server_name: server.name.clone(),
                    enable: false,
                }
            }
            McpServerUsabilityState::Disabled | McpServerUsabilityState::Failed => {
                McpToggleRequest {
                    server_name: server.name.clone(),
                    enable: true,
                }
            }
        };

        self.pending_mcp_toggle_requests.push_back(request);
        true
    }

    pub fn take_next_mcp_toggle_request(&mut self) -> Option<McpToggleRequest> {
        self.pending_mcp_toggle_requests.pop_front()
    }

    pub fn mcp_counts(&self) -> (usize, usize, usize, usize) {
        let configured = self.mcp_servers.len();
        let enabled = self
            .mcp_servers
            .iter()
            .filter(|s| s.state == McpServerUsabilityState::Enabled)
            .count();
        let disabled = self
            .mcp_servers
            .iter()
            .filter(|s| s.state == McpServerUsabilityState::Disabled)
            .count();
        let failed = self
            .mcp_servers
            .iter()
            .filter(|s| s.state == McpServerUsabilityState::Failed)
            .count();
        (configured, enabled, disabled, failed)
    }

    pub fn command_palette_move_up(&mut self) {
        let len = self.command_palette_actions().len();
        if len == 0 {
            self.command_palette_selection = 0;
            return;
        }

        self.command_palette_selection = if self.command_palette_selection == 0 {
            len.saturating_sub(1)
        } else {
            self.command_palette_selection.saturating_sub(1)
        };
    }

    pub fn command_palette_move_down(&mut self) {
        let len = self.command_palette_actions().len();
        if len == 0 {
            self.command_palette_selection = 0;
            return;
        }

        self.command_palette_selection = (self.command_palette_selection + 1) % len;
    }

    pub fn append_command_palette_query_char(&mut self, ch: char) {
        self.command_palette_query.push(ch);
        self.command_palette_selection = 0;
    }

    pub fn backspace_command_palette_query_char(&mut self) {
        self.command_palette_query.pop();
        self.command_palette_selection = 0;
    }

    pub fn prompt_status_for_transcript_line(
        &self,
        transcript_line_index: usize,
    ) -> Option<PromptStatus> {
        self.prompt_items
            .iter()
            .rev()
            .find(|prompt| prompt.transcript_line_index == transcript_line_index)
            .map(|prompt| prompt.status)
    }

    pub fn transcript_line_status_for_index(
        &self,
        transcript_line_index: usize,
    ) -> Option<TranscriptLineStatus> {
        if let Some(status) = self.prompt_status_for_transcript_line(transcript_line_index) {
            return Some(TranscriptLineStatus::Prompt(status));
        }

        if let Some(status) = self
            .compaction_items
            .iter()
            .rev()
            .find(|item| item.transcript_line_index == transcript_line_index)
            .map(|item| item.status)
        {
            return Some(TranscriptLineStatus::Compaction(status));
        }

        self.tool_call_items
            .iter()
            .rev()
            .find(|tool| tool.transcript_line_index == transcript_line_index)
            .map(|tool| TranscriptLineStatus::Tool(tool.status))
    }

    pub fn start_compaction_block(&mut self, source: &str) {
        if self
            .compaction_items
            .iter()
            .any(|item| item.source == source && item.status == CompactionStatus::InProgress)
        {
            return;
        }
        if !self.transcript_preview.is_empty() {
            self.push_transcript_line(TranscriptRole::Separator, String::new());
        }
        self.push_transcript_line(TranscriptRole::System, "Compaction".to_string());
        let transcript_line_index = self.transcript_preview.len().saturating_sub(1);
        self.compaction_items.push(CompactionLine {
            transcript_line_index,
            source: source.to_string(),
            status: CompactionStatus::InProgress,
        });
    }

    pub fn finish_compaction_block(&mut self, source: &str, status: CompactionStatus) {
        if let Some(item) = self
            .compaction_items
            .iter_mut()
            .rev()
            .find(|item| item.source == source && item.status == CompactionStatus::InProgress)
        {
            item.status = status;
            return;
        }

        if let Some(item) = self
            .compaction_items
            .iter_mut()
            .rev()
            .find(|item| item.status == CompactionStatus::InProgress)
        {
            item.status = status;
        }
    }

    pub fn start_tool_call(&mut self, name: &str, arguments: &str) {
        let args_summary = crate::agent::protocol::tool_args::summarize_tool_arguments(arguments);
        let line_text = format!("tool[{name}] args={args_summary}");
        self.push_transcript_line(TranscriptRole::Tool, line_text);

        let transcript_line_index = self.transcript_preview.len().saturating_sub(1);
        tool_calls::ToolCallBookkeeping::new(
            &mut self.tool_call_items,
            &mut self.active_tool_ids_by_key,
            &mut self.next_tool_call_id,
        )
        .start_tool_call(transcript_line_index, name, arguments);
    }

    pub fn finish_tool_call(&mut self, name: &str, arguments: &str, success: bool) {
        tool_calls::ToolCallBookkeeping::new(
            &mut self.tool_call_items,
            &mut self.active_tool_ids_by_key,
            &mut self.next_tool_call_id,
        )
        .finish_tool_call(name, arguments, success);
    }

    pub fn append_input_char(&mut self, ch: char) {
        if self.input.locked {
            self.ensure_invariants();
            return;
        }

        if self.input.cursor >= self.input.buffer.len() {
            self.input.buffer.push(ch);
            self.input.cursor = self.input.buffer.len();
        } else {
            self.input.buffer.insert(self.input.cursor, ch);
            self.input.cursor += ch.len_utf8();
        }

        self.ensure_invariants();
    }

    pub fn insert_input_newline(&mut self) {
        self.append_input_char('\n');
    }

    pub fn enter_insert_mode(&mut self) {
        self.input_mode = InputMode::Insert;
        self.insert_exit_pending_j = false;
        self.normal_pending_key = None;
        self.pane_focus = PaneFocus::Input;
    }

    pub fn enter_normal_mode(&mut self) {
        self.input_mode = InputMode::Normal;
        self.insert_exit_pending_j = false;
        self.normal_pending_key = None;
        self.pane_focus = PaneFocus::Transcript;
    }

    pub fn set_insert_exit_pending_j(&mut self, pending: bool) {
        self.insert_exit_pending_j = pending;
    }

    pub fn insert_exit_pending_j(&self) -> bool {
        self.insert_exit_pending_j
    }

    pub fn clear_normal_pending_key(&mut self) {
        self.normal_pending_key = None;
    }

    pub fn arm_normal_pending_key(&mut self, key: char) {
        self.normal_pending_key = Some(key);
    }

    pub fn take_normal_pending_key_if(&mut self, key: char) -> bool {
        let matches = self.normal_pending_key == Some(key);
        self.normal_pending_key = None;
        matches
    }

    pub fn transcript_cursor_index(&self) -> Option<usize> {
        self.transcript_list_state.selected
    }

    pub fn take_clipboard_request(&mut self) -> Option<String> {
        self.clipboard_request.take()
    }

    pub fn backspace_input_char(&mut self) {
        if self.input.locked {
            self.ensure_invariants();
            return;
        }

        if let Some(start) = previous_char_start(&self.input.buffer, self.input.cursor) {
            self.input.buffer.drain(start..self.input.cursor);
            self.input.cursor = start;
        }

        self.ensure_invariants();
    }

    pub fn delete_input_char(&mut self) {
        if self.input.locked {
            self.ensure_invariants();
            return;
        }

        if let Some(end) = next_char_end(&self.input.buffer, self.input.cursor) {
            self.input.buffer.drain(self.input.cursor..end);
        }

        self.ensure_invariants();
    }

    pub fn move_cursor_left(&mut self) {
        if self.input.locked {
            self.ensure_invariants();
            return;
        }

        if let Some(start) = previous_char_start(&self.input.buffer, self.input.cursor) {
            self.input.cursor = start;
        }

        self.ensure_invariants();
    }

    pub fn move_cursor_right(&mut self) {
        if self.input.locked {
            self.ensure_invariants();
            return;
        }

        if let Some(end) = next_char_end(&self.input.buffer, self.input.cursor) {
            self.input.cursor = end;
        }

        self.ensure_invariants();
    }

    pub fn move_cursor_home(&mut self) {
        if !self.input.locked {
            self.input.cursor = 0;
        }
        self.ensure_invariants();
    }

    pub fn move_cursor_end(&mut self) {
        if !self.input.locked {
            self.input.cursor = self.input.buffer.len();
        }
        self.ensure_invariants();
    }

    pub fn accept_submit(&mut self) {
        self.input.buffer.clear();
        self.input.cursor = 0;
        self.phase = UiPhase::Busy;
        self.active_cycle = self.active_prompt_id.is_some() || !self.pending_prompt_ids.is_empty();
        self.abort.pending = false;
        self.ensure_invariants();
    }

    pub fn enqueue_external_prompt(&mut self, text: String) {
        self.push_transcript_line(TranscriptRole::User, text.clone());
        let transcript_line_index = self.transcript_preview.len().saturating_sub(1);
        let id = self.next_prompt_id;
        self.next_prompt_id = self.next_prompt_id.saturating_add(1);
        self.prompt_items.push(QueuedPrompt {
            id,
            prompt_text: text,
            transcript_line_index,
            status: PromptStatus::InProgress,
        });
        self.active_prompt_id = Some(id);
        self.phase = UiPhase::Busy;
        self.active_cycle = true;
    }

    pub fn enqueue_prompt(&mut self, submitted_text: String) -> u64 {
        self.push_transcript_line(TranscriptRole::User, submitted_text.clone());
        let transcript_line_index = self.transcript_preview.len().saturating_sub(1);
        let id = prompt_queue::PromptQueueLifecycle::new(
            &mut self.prompt_items,
            &mut self.pending_prompt_ids,
            &mut self.active_prompt_id,
            &mut self.next_prompt_id,
        )
        .enqueue_prompt(submitted_text, transcript_line_index);
        self.accept_submit();
        id
    }

    pub fn enqueue_immediate_submission(&mut self, submitted_text: String) {
        self.pending_immediate_submissions.push_back(submitted_text);
        self.input.buffer.clear();
        self.input.cursor = 0;
        self.abort.pending = false;
        self.ensure_invariants();
    }

    pub fn activate_next_prompt(&mut self) -> Option<u64> {
        let maybe_id = prompt_queue::PromptQueueLifecycle::new(
            &mut self.prompt_items,
            &mut self.pending_prompt_ids,
            &mut self.active_prompt_id,
            &mut self.next_prompt_id,
        )
        .activate_next_prompt();

        if maybe_id.is_none() {
            self.ensure_invariants();
            return None;
        }

        self.phase = UiPhase::Busy;
        self.active_cycle = true;
        self.abort.pending = false;
        self.ensure_invariants();
        maybe_id
    }

    pub fn take_next_prompt_for_execution(&mut self) -> Option<String> {
        if let Some(immediate) = self.pending_immediate_submissions.pop_front() {
            self.ensure_invariants();
            return Some(immediate);
        }

        let active_id = self.activate_next_prompt()?;
        self.prompt_items
            .iter()
            .find(|prompt| prompt.id == active_id)
            .map(|prompt| prompt.prompt_text.clone())
    }

    pub fn complete_active_prompt(&mut self) {
        prompt_queue::PromptQueueLifecycle::new(
            &mut self.prompt_items,
            &mut self.pending_prompt_ids,
            &mut self.active_prompt_id,
            &mut self.next_prompt_id,
        )
        .complete_active_prompt();

        self.phase = UiPhase::Idle;
        self.active_cycle = false;
        self.abort.pending = false;
        self.ensure_invariants();
    }

    pub fn cancel_active_and_pending_prompts(&mut self) {
        prompt_queue::PromptQueueLifecycle::new(
            &mut self.prompt_items,
            &mut self.pending_prompt_ids,
            &mut self.active_prompt_id,
            &mut self.next_prompt_id,
        )
        .cancel_active_and_pending_prompts();

        self.phase = UiPhase::Idle;
        self.active_cycle = false;
        self.abort.pending = false;
        self.ensure_invariants();
    }

    pub fn request_abort_confirmation(&mut self) -> bool {
        if !(self.active_prompt_id.is_some()
            || !self.pending_prompt_ids.is_empty()
            || self.active_cycle)
        {
            self.ensure_invariants();
            return false;
        }

        self.abort.pending = true;
        self.abort.confirmation_marker = self.abort.confirmation_marker.saturating_add(1);
        self.phase = UiPhase::AbortPending;
        self.ensure_invariants();
        true
    }

    pub fn finalize_cycle(&mut self) {
        self.complete_active_prompt();
    }

    pub fn push_transcript_line(&mut self, role: TranscriptRole, line: impl Into<String>) {
        let text = line.into();
        let entry = match role {
            TranscriptRole::User => TranscriptEntry::User(UserMessage { text }),
            TranscriptRole::Assistant => TranscriptEntry::Assistant(AssistantChunk {
                lines: vec![ContentLine::single(text, StyleHint::Normal)],
            }),
            TranscriptRole::Tool => {
                let (name, args) = parse_tool_text(&text);
                TranscriptEntry::Tool(ToolInvocation {
                    name,
                    source: String::new(),
                    args,
                })
            }
            TranscriptRole::ToolDisplay => TranscriptEntry::ToolResult(TranscriptToolResult {
                name: String::new(),
                success: true,
                lines: vec![DisplayLine::new(text.clone(), annotate_diff_hint(&text))],
            }),
            TranscriptRole::Compaction => TranscriptEntry::System(SystemMessage { text }),
            TranscriptRole::System => TranscriptEntry::System(SystemMessage { text }),
            TranscriptRole::Separator => TranscriptEntry::Separator(TranscriptSeparator),
        };
        self.push_transcript_item(entry);
    }

    pub fn push_transcript_rendered_line(&mut self, role: TranscriptRole, line: Line<'static>) {
        match role {
            TranscriptRole::Assistant | TranscriptRole::Compaction => {
                let content_line = ratatui_line_to_content_line(&line);
                let entry = TranscriptEntry::Assistant(AssistantChunk {
                    lines: vec![content_line],
                });
                self.push_transcript_item(entry);
            }
            _ => {
                let text = rendered_line_to_plain_text(&line);
                self.push_transcript_line(role, text);
            }
        }
    }

    pub fn project_assistant_markdown_lines(&mut self, markdown: &str) -> Vec<Line<'static>> {
        if let Some(cached) = self.assistant_projection_cache.get(markdown) {
            return cached.clone();
        }

        let projected = project_markdown_to_lines(markdown);
        self.assistant_projection_cache
            .insert(markdown.to_string(), projected.clone());
        #[cfg(test)]
        {
            self.assistant_projection_cache_misses =
                self.assistant_projection_cache_misses.saturating_add(1);
        }
        projected
    }

    pub fn clear_assistant_projection_cache(&mut self) {
        self.assistant_projection_cache.clear();
    }

    #[cfg(test)]
    pub fn assistant_projection_cache_size(&self) -> usize {
        self.assistant_projection_cache.len()
    }

    #[cfg(test)]
    pub fn assistant_projection_cache_misses(&self) -> usize {
        self.assistant_projection_cache_misses
    }

    fn push_transcript_item(&mut self, entry: TranscriptEntry) {
        // Check if we should follow tail (user is at end, or nothing selected)
        let was_at_end = match self.transcript_list_state.selected {
            Some(idx) => idx + 1 >= self.transcript_preview.len(),
            None => true,
        };

        let entry_role = entry.role();
        if should_insert_turn_separator(
            self.transcript_preview.last().map(|e| e.role()).as_ref(),
            &entry_role,
        ) {
            self.transcript_preview
                .push(TranscriptEntry::Separator(TranscriptSeparator));
        }

        // Visual spacer between different roles (checks previous role AFTER separator may have been inserted)
        if needs_spacer(
            self.transcript_preview.last().map(|e| e.role()).as_ref(),
            &entry_role,
        ) {
            self.transcript_preview
                .push(TranscriptEntry::Spacer(SpacerItem));
        }

        self.transcript_preview.push(entry);

        // Only follow tail if user was already at the end
        if was_at_end {
            self.transcript_list_state
                .select(Some(self.transcript_preview.len().saturating_sub(1)));
        }
    }

    pub fn scroll_transcript_page_up(&mut self, page_lines: usize) {
        let current = self.transcript_list_state.selected.unwrap_or(0);
        self.transcript_list_state
            .select(Some(current.saturating_sub(page_lines.max(1))));
    }

    pub fn scroll_transcript_line_up(&mut self) {
        let current = self.transcript_list_state.selected.unwrap_or(0);
        self.transcript_list_state
            .select(Some(current.saturating_sub(1)));
    }

    pub fn scroll_transcript_page_down(&mut self, page_lines: usize) {
        let current = self.transcript_list_state.selected.unwrap_or(0);
        let last = self.transcript_preview.len().saturating_sub(1);
        self.transcript_list_state
            .select(Some(current.saturating_add(page_lines.max(1)).min(last)));
    }

    pub fn scroll_transcript_line_down(&mut self) {
        let current = self.transcript_list_state.selected.unwrap_or(0);
        let last = self.transcript_preview.len().saturating_sub(1);
        self.transcript_list_state
            .select(Some(current.saturating_add(1).min(last)));
    }

    pub fn scroll_transcript_to_top(&mut self) {
        self.transcript_list_state.select(Some(0));
    }

    pub fn scroll_transcript_to_bottom(&mut self) {
        let last = self.transcript_preview.len().saturating_sub(1);
        self.transcript_list_state.select(Some(last));
    }

    pub fn focus_prev_pane(&mut self) {
        self.pane_focus = match self.pane_focus {
            PaneFocus::Transcript => PaneFocus::Input,
            PaneFocus::Input => PaneFocus::Transcript,
        };
    }

    pub fn focus_next_pane(&mut self) {
        self.pane_focus = match self.pane_focus {
            PaneFocus::Transcript => PaneFocus::Input,
            PaneFocus::Input => PaneFocus::Transcript,
        };
    }

    pub fn request_quit_if_idle(&mut self) {
        if self.phase == UiPhase::Idle && self.input.buffer.is_empty() {
            self.quit_requested = true;
        }
    }

    pub fn record_token_usage(&mut self, input_tokens: u64, output_tokens: u64, total_tokens: u64) {
        self.latest_input_tokens = Some(input_tokens);
        self.latest_output_tokens = Some(output_tokens);
        self.latest_total_tokens = Some(total_tokens);
    }

    pub fn hydrate_latest_total_tokens(&mut self, total_tokens: u64) {
        self.latest_total_tokens = Some(total_tokens);
    }

    pub fn hydrate_usage(
        &mut self,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
        total_tokens: Option<u64>,
    ) {
        if let Some(input_tokens) = input_tokens {
            self.latest_input_tokens = Some(input_tokens);
        }
        if let Some(output_tokens) = output_tokens {
            self.latest_output_tokens = Some(output_tokens);
        }
        if let Some(total_tokens) = total_tokens {
            self.hydrate_latest_total_tokens(total_tokens);
        }
    }

    pub fn set_context_window_max_tokens(&mut self, max_tokens: Option<u64>) {
        self.context_window_max_tokens = max_tokens;
    }

    pub fn context_window_max_tokens(&self) -> Option<u64> {
        self.context_window_max_tokens
    }

    pub fn ensure_invariants(&mut self) {
        if self.phase == UiPhase::AbortPending && !self.active_cycle {
            self.phase = UiPhase::Idle;
            self.abort.pending = false;
        }

        if self.phase != UiPhase::AbortPending {
            self.abort.pending = false;
        }

        self.active_cycle = self.active_prompt_id.is_some() || !self.pending_prompt_ids.is_empty();

        self.inline_slash_commands = filter_inline_slash_suggestions(&self.input.buffer);
        self.inline_slash_open = !self.inline_slash_commands.is_empty();
        if !self.inline_slash_open {
            self.inline_slash_selection = 0;
        } else if self.inline_slash_selection >= self.inline_slash_commands.len() {
            self.inline_slash_selection = self.inline_slash_commands.len().saturating_sub(1);
        }

        if self.phase == UiPhase::Idle && self.active_cycle {
            self.phase = UiPhase::Busy;
        }

        if self.phase == UiPhase::AbortPending && !self.active_cycle {
            self.phase = UiPhase::Idle;
            self.abort.pending = false;
        }

        if self.input.cursor > self.input.buffer.len() {
            self.input.cursor = self.input.buffer.len();
        }

        let palette_len = self.command_palette_actions().len();
        if palette_len == 0 {
            self.command_palette_selection = 0;
        } else if self.command_palette_selection >= palette_len {
            self.command_palette_selection = palette_len.saturating_sub(1);
        }

        if self.mcp_servers.is_empty() {
            self.mcp_panel_selection = 0;
        } else if self.mcp_panel_selection >= self.mcp_servers.len() {
            self.mcp_panel_selection = self.mcp_servers.len().saturating_sub(1);
        }

        if !self.model_picker_open {
            self.model_picker_selection = 0;
            self.model_picker_query.clear();
        } else {
            let len = self.model_picker_filtered_options().len();
            if len == 0 {
                self.model_picker_selection = 0;
            } else if self.model_picker_selection >= len {
                self.model_picker_selection = len.saturating_sub(1);
            }
        }

        if !self.agent_picker_open {
            self.agent_picker_selection = 0;
            self.agent_picker_query.clear();
        } else {
            let agent_filtered_count = self.agent_picker_filtered_options().len();
            if agent_filtered_count == 0 {
                self.agent_picker_selection = 0;
            } else if self.agent_picker_selection >= agent_filtered_count {
                self.agent_picker_selection = agent_filtered_count.saturating_sub(1);
            }
        }

        while self.input.cursor > 0 && !self.input.buffer.is_char_boundary(self.input.cursor) {
            self.input.cursor -= 1;
        }

        // With ListState, viewport invariants are managed by ratatui automatically

        self.input.locked = false;

        prompt_queue::PromptQueueLifecycle::new(
            &mut self.prompt_items,
            &mut self.pending_prompt_ids,
            &mut self.active_prompt_id,
            &mut self.next_prompt_id,
        )
        .enforce_single_active_invariant();
    }
}

pub fn info_panel_for_command_palette_action(action: CommandPaletteAction) -> Option<InfoPanel> {
    match action {
        CommandPaletteAction::Compact => None,
        CommandPaletteAction::Help => Some(InfoPanel::Help),
        CommandPaletteAction::Status => Some(InfoPanel::Status),
        CommandPaletteAction::Mcps => Some(InfoPanel::Mcps),
        CommandPaletteAction::Skills => Some(InfoPanel::Skills),
        CommandPaletteAction::Models => None,
        CommandPaletteAction::Agents => None,
    }
}

fn fuzzy_matches(query: &str, candidate: &str) -> bool {
    if query.is_empty() {
        return true;
    }

    let mut query_chars = query.chars();
    let mut needle = query_chars.next();
    for ch in candidate.chars() {
        if Some(ch) == needle {
            needle = query_chars.next();
            if needle.is_none() {
                return true;
            }
        }
    }
    false
}

fn should_insert_turn_separator(previous: Option<&Role>, next: &Role) -> bool {
    matches!(
        (previous, next),
        (Some(prev), next) if is_turn_role(prev) && is_turn_role(next) && prev != next
    )
}

fn is_turn_role(role: &Role) -> bool {
    matches!(role, Role::User | Role::Assistant | Role::Tool)
}

pub(super) fn needs_spacer(previous: Option<&Role>, next: &Role) -> bool {
    let Some(previous) = previous else {
        return false;
    };
    if previous == next {
        return false;
    }
    if *previous == Role::Separator || *next == Role::Separator {
        return false;
    }
    !matches!(
        (previous, next),
        (Role::User, Role::Assistant)
            | (Role::Assistant, Role::User)
            | (Role::Tool, Role::ToolDisplay)
            | (Role::ToolDisplay, Role::Tool)
    )
}

fn previous_char_start(buffer: &str, cursor: usize) -> Option<usize> {
    if cursor == 0 {
        return None;
    }

    let cursor = cursor.min(buffer.len());
    buffer[..cursor].char_indices().last().map(|(idx, _)| idx)
}

fn next_char_end(buffer: &str, cursor: usize) -> Option<usize> {
    if cursor >= buffer.len() {
        return None;
    }

    let cursor = cursor.min(buffer.len());
    buffer[cursor..]
        .chars()
        .next()
        .map(|ch| cursor + ch.len_utf8())
}

fn ratatui_line_to_content_line(line: &Line<'static>) -> ContentLine {
    let spans = line
        .spans
        .iter()
        .map(|span| Span {
            text: span.content.to_string(),
            hint: StyleHint::Rendered(span.style),
        })
        .collect();
    ContentLine { spans }
}
