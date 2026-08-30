//! Application state types and the `AppState` reducer root.

use std::collections::{HashMap, VecDeque};

use nu_agent_core::orchestrator::{OrchestratorEvent, UiRequest, UiStateEvent};
use nu_agent_core::protocol::contracts::{McpUsabilityState, SharedUiAction, UiMessageSnapshot};
use nu_agent_core::protocol::event::{PermissionDecisionSubmission, UiEvent};
use nu_agent_core::protocol::slash::SlashCommand;
use nu_agent_core::transcript::ir::{ContentLine, Role};
use nu_agent_core::transcript::items::{ProseMessage, TranscriptEntry, TranscriptEntryKind};

use crate::interaction::cancel::CancelController;
use crate::interaction::dispatch::rewrite_action;
use crate::interaction::input::{TerminalEvent, map_terminal_event};
use crate::interaction::reducer::{
    ReducerInput, append_direct_tool_display, reduce_with_cancel_controller,
};
use crate::rendering::theme::{ThemeName, TuiTheme};
use crate::state::selection::TranscriptSelection;
use crate::state::tool_parsing::{extract_tool_name, parse_persisted_tool_status_line};
use nu_agent_core::protocol::picker::{AgentPickerOption, ModelPickerOption};

const STARTUP_LOGOS: &[&str] = &[
    include_str!("../logos/00.txt"),
    include_str!("../logos/01.txt"),
    include_str!("../logos/02.txt"),
    include_str!("../logos/03.txt"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryVisualInfo {
    pub start_visual_row: usize,
    pub visual_row_count: usize,
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionLine {
    pub source: String,
    pub status: CompactionStatus,
    pub entry_id: Option<u64>,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionPickerOption {
    pub id: String,
    pub title: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub display: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemePickerOption {
    pub name: String,
    pub display_name: String,
    pub active: bool,
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
    pub entry_id: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCallLine {
    pub id: u64,
    pub status: ToolCallStatus,
    pub key: String,
    pub entry_id: Option<u64>,
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
pub struct AbortState {
    pub pending: bool,
    pub confirmation_marker: u64,
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub phase: UiPhase,
    pub input_locked: bool,
    pub abort: AbortState,
    pub transcript_preview: Vec<TranscriptEntry>,
    pub transcript_scroll_offset: usize,
    pub transcript_following_tail: bool,
    pub status_line: String,
    pub input_mode: InputMode,
    pub pane_focus: PaneFocus,
    pub latest_input_tokens: Option<u64>,
    pub latest_output_tokens: Option<u64>,
    pub latest_total_tokens: Option<u64>,
    pub session_total_tokens: u64,
    pub(crate) context_window_max_tokens: Option<u64>,
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
    pub session_picker_open: bool,
    pub session_picker_query: String,
    pub session_picker_selection: usize,
    pub session_picker_options: Vec<SessionPickerOption>,
    pub theme_picker_open: bool,
    pub theme_picker_selection: usize,
    pub theme_picker_options: Vec<ThemePickerOption>,
    pub(crate) pending_agent_picker_launch_requests: usize,
    pub(crate) pending_agent_switch_requests: VecDeque<String>,
    pub(crate) active_agent_identity: Option<String>,
    pub active_model_identity: String,
    pub active_persona_icon: Option<String>,
    pub agent_cycle_names: Vec<String>,
    pub info_panel: Option<InfoPanel>,
    pub info_panel_scroll: usize,
    pub mcp_servers: Vec<McpServerState>,
    pub mcp_panel_selection: usize,
    pub(crate) discoverable_skills: Vec<DiscoverableSkill>,
    pub(crate) skills_discovery_failed: bool,
    pub(crate) llm_visible_mcp_tool_count: usize,
    pub(crate) mcp_visible_tool_count_by_server: HashMap<String, usize>,
    pub(crate) mcp_visible_tool_names_by_server: HashMap<String, Vec<String>>,
    pub(crate) mcp_failure_reasons: HashMap<String, String>,
    pub(crate) pending_mcp_toggle_requests: VecDeque<McpToggleRequest>,
    pub(crate) pending_model_switch_requests: VecDeque<String>,
    pub(crate) pending_permission_decisions: VecDeque<PermissionDecisionSubmission>,
    pub(crate) pending_model_picker_launch_requests: usize,
    pub(crate) pending_session_picker_launch_requests: usize,
    pub(crate) pending_session_switch_requests: VecDeque<String>,
    pub(crate) pending_theme_picker_launch_requests: usize,
    pub(crate) pending_theme_switch_requests: VecDeque<String>,
    pub permission_prompt: Option<PermissionPrompt>,
    pub(crate) assistant_projection_cache: HashMap<String, Vec<ContentLine>>,
    pub(crate) prompt_items: Vec<QueuedPrompt>,
    pub(crate) tool_call_items: Vec<ToolCallLine>,
    pub(crate) compaction_items: Vec<CompactionLine>,
    pub(crate) active_tool_ids_by_key: HashMap<String, VecDeque<u64>>,
    pub(crate) pending_prompt_ids: VecDeque<u64>,
    pub(crate) pending_immediate_submissions: VecDeque<String>,
    pub(crate) active_prompt_id: Option<u64>,
    pub(crate) next_prompt_id: u64,
    pub(crate) next_tool_call_id: u64,
    pub(crate) next_entry_id: u64,
    pub(crate) active_cycle: bool,
    pub(crate) insert_exit_pending_j: Option<std::time::Instant>,
    pub(crate) normal_pending_key: Option<char>,
    pub(crate) input_history_index: Option<usize>,
    pub(crate) input_history_saved: String,
    pub(crate) inline_slash_commands: Vec<SlashCommand>,
    pub(crate) clipboard_request: Option<String>,
    pub pending_submit_text: Option<String>,
    /// Text restored from cancelled pending prompts, to be set on the textarea
    /// by the coordinator after the next pump cycle.
    pub restored_input_text: Option<String>,
    pub transcript_selection: Option<TranscriptSelection>,
    pub cursor_visual_row: usize,
    pub viewport_height: usize,
    pub max_scroll: usize,
    pub entry_indices: Vec<usize>,
    pub total_visual_rows: usize,
    pub pre_displayed_tool_keys: std::collections::HashSet<String>,
    pub rendered_line_text: Vec<String>,
    pub rendered_line_start_row: usize,
    pub(crate) streaming_message_start: Option<usize>,
    pub(crate) compaction_streaming_start: Option<usize>,
    pub entry_visual_info: Vec<EntryVisualInfo>,
    pub entry_visual_info_dirty: bool,
    pub theme: TuiTheme,
    pub event_tx: tokio::sync::mpsc::Sender<nu_agent_core::orchestrator::OrchestratorEvent>,
}

impl Default for AppState {
    fn default() -> Self {
        let (event_tx, _event_rx) =
            tokio::sync::mpsc::channel::<nu_agent_core::orchestrator::OrchestratorEvent>(256);
        Self {
            phase: UiPhase::Idle,
            input_locked: false,
            abort: AbortState::default(),
            transcript_preview: Vec::new(),
            transcript_scroll_offset: 0,
            transcript_following_tail: true,
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
            session_picker_open: false,
            session_picker_query: String::new(),
            session_picker_selection: 0,
            session_picker_options: Vec::new(),
            theme_picker_open: false,
            theme_picker_selection: 0,
            theme_picker_options: Vec::new(),
            pending_agent_picker_launch_requests: 0,
            pending_agent_switch_requests: VecDeque::new(),
            active_agent_identity: None,
            active_model_identity: String::new(),
            active_persona_icon: None,
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
            pending_session_picker_launch_requests: 0,
            pending_session_switch_requests: VecDeque::new(),
            pending_theme_picker_launch_requests: 0,
            pending_theme_switch_requests: VecDeque::new(),
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
            next_entry_id: 1,
            active_cycle: false,
            insert_exit_pending_j: None,
            normal_pending_key: None,
            input_history_index: None,
            input_history_saved: String::new(),
            inline_slash_commands: Vec::new(),
            clipboard_request: None,
            pending_submit_text: None,
            restored_input_text: None,
            transcript_selection: None,
            cursor_visual_row: 0,
            viewport_height: 0,
            max_scroll: 0,
            entry_indices: Vec::new(),
            total_visual_rows: 0,
            pre_displayed_tool_keys: std::collections::HashSet::new(),
            rendered_line_text: Vec::new(),
            rendered_line_start_row: 0,
            streaming_message_start: None,
            compaction_streaming_start: None,
            entry_visual_info: Vec::new(),
            entry_visual_info_dirty: true,
            theme: TuiTheme::default(),
            event_tx,
        }
    }
}

impl AppState {
    pub fn take_pending_events(
        &mut self,
        cancel_controller: &CancelController,
    ) -> Vec<OrchestratorEvent> {
        let mut events = Vec::new();
        while let Some(prompt) = self.take_next_prompt_for_execution() {
            events.push(OrchestratorEvent::PromptSubmitted { text: prompt });
        }
        while let Some(decision) = self.take_next_permission_decision_submission() {
            events.push(OrchestratorEvent::PermissionDecision { decision });
        }
        while let Some(spec) = self.take_next_model_switch_request() {
            events.push(OrchestratorEvent::UiRequest(UiRequest::SwitchModel {
                spec,
            }));
        }
        while let Some(name) = self.take_next_agent_switch_request() {
            events.push(OrchestratorEvent::UiRequest(UiRequest::SwitchAgent {
                name,
            }));
        }
        while let Some(id) = self.take_next_session_switch_request() {
            events.push(OrchestratorEvent::UiRequest(UiRequest::SwitchSession {
                id,
            }));
        }
        while let Some(req) = self.take_next_mcp_toggle_request() {
            events.push(OrchestratorEvent::UiRequest(UiRequest::ToggleMcp {
                server: req.server_name,
                enable: req.enable,
            }));
        }
        if cancel_controller.take_cancel_requested() {
            events.push(OrchestratorEvent::CancelRequested);
        }
        events
    }

    pub fn take_pending_ui_state_events(&mut self) -> Vec<UiStateEvent> {
        let mut events = Vec::new();
        if self.take_next_model_picker_launch_request() {
            events.push(UiStateEvent::ExecuteSharedUiAction(SharedUiAction::Models));
        }
        if self.take_next_agent_picker_launch_request() {
            events.push(UiStateEvent::ExecuteSharedUiAction(SharedUiAction::Agents));
        }
        if self.take_next_session_picker_launch_request() {
            events.push(UiStateEvent::ExecuteSharedUiAction(
                SharedUiAction::Sessions,
            ));
        }
        if self.take_next_theme_picker_launch_request() {
            events.push(UiStateEvent::ExecuteSharedUiAction(SharedUiAction::Themes));
        }
        events
    }

    pub fn reduce_terminal_event(
        &mut self,
        event: &TerminalEvent,
        cancel_controller: Option<&CancelController>,
    ) -> bool {
        let Some(mapped_action) = map_terminal_event(event, self.input_locked) else {
            return false;
        };
        let (action, force_changed) = rewrite_action(self, mapped_action);
        let changed =
            reduce_with_cancel_controller(self, ReducerInput::User(action), cancel_controller);
        force_changed || changed
    }

    pub fn reduce_ui_event(&mut self, event: UiEvent) -> bool {
        crate::interaction::reducer::reduce_ui_event_impl(self, event)
    }

    pub fn reduce_ui_state_event(&mut self, event: UiStateEvent) {
        match event {
            UiStateEvent::SetActiveModelIdentity(s) => self.set_active_model_identity(&s),
            UiStateEvent::SetActiveAgentIdentity(s) => self.set_active_agent_identity(&s),
            UiStateEvent::SetActivePersonaIcon(icon) => self.set_active_persona_icon(icon),
            UiStateEvent::SetContextWindowMaxTokens(tokens) => {
                self.set_context_window_max_tokens(tokens)
            }
            UiStateEvent::ClearTranscript => self.clear_transcript(),
            UiStateEvent::HydrateTranscript {
                messages,
                last_total_tokens,
            } => self.hydrate_transcript_from_messages(messages, last_total_tokens),
            UiStateEvent::SetMcpServerState {
                server,
                state,
                error,
                total,
            } => {
                let mapped = match state {
                    McpUsabilityState::Enabled => McpServerUsabilityState::Enabled,
                    McpUsabilityState::Disabled => McpServerUsabilityState::Disabled,
                    McpUsabilityState::Failed => McpServerUsabilityState::Failed,
                };
                self.set_mcp_server_state_with_details(&server, mapped, error, total)
            }
            UiStateEvent::SetMcpVisibleToolCount { server, count } => {
                self.set_mcp_visible_tool_count_by_server_name(&server, count)
            }
            UiStateEvent::SetMcpVisibleToolNames { server, names } => {
                self.set_mcp_visible_tool_names_by_server_name(&server, names)
            }
            UiStateEvent::SetSessionPickerOptions(sessions) => {
                let tui_options: Vec<SessionPickerOption> = sessions
                    .into_iter()
                    .map(|info| {
                        let display = info
                            .title
                            .clone()
                            .unwrap_or_else(|| "(untitled)".to_string());
                        SessionPickerOption {
                            id: info.id,
                            title: info.title,
                            created_at: info.last_active,
                            display,
                        }
                    })
                    .collect();
                self.set_session_picker_options(tui_options)
            }
            UiStateEvent::DisplayIncomingMessage(msg) => self.display_incoming_message(&msg),
            UiStateEvent::ExecuteSharedUiAction(action) => {
                self.execute_shared_ui_action(action);
            }
            UiStateEvent::PushStartupLogo => {
                self.push_startup_logo();
            }
        }
    }

    fn set_active_model_identity(&mut self, identity: &str) {
        self.active_model_identity = identity.to_string();
    }

    fn set_active_persona_icon(&mut self, icon: Option<String>) {
        self.active_persona_icon = icon;
    }

    fn set_mcp_server_state_with_details(
        &mut self,
        server_name: &str,
        state: McpServerUsabilityState,
        reason: Option<String>,
        llm_visible_mcp_tool_count: usize,
    ) {
        self.set_llm_visible_mcp_tool_count(llm_visible_mcp_tool_count);
        self.set_mcp_server_state_by_name_with_reason(server_name, state, reason);
    }

    fn display_incoming_message(&mut self, text: &str) {
        self.enqueue_external_prompt(text.to_string());
    }

    fn execute_shared_ui_action(&mut self, action: SharedUiAction) -> bool {
        match action {
            SharedUiAction::Help => {
                self.open_info_panel(InfoPanel::Help);
                true
            }
            SharedUiAction::Status => {
                self.open_info_panel(InfoPanel::Status);
                true
            }
            SharedUiAction::Mcps => {
                self.open_info_panel(InfoPanel::Mcps);
                true
            }
            SharedUiAction::Models => {
                self.open_model_picker();
                true
            }
            SharedUiAction::Agents => {
                self.open_agent_picker();
                true
            }
            SharedUiAction::Sessions => {
                self.open_session_picker();
                true
            }
            SharedUiAction::Themes => {
                let current = ThemeName::all()
                    .into_iter()
                    .find(|name| name.resolve() == self.theme)
                    .unwrap_or_default();
                let options = ThemeName::all()
                    .into_iter()
                    .map(|name| ThemePickerOption {
                        name: format!("{name:?}"),
                        display_name: match name {
                            ThemeName::CatppuccinMocha => "Catppuccin Mocha".to_string(),
                            ThemeName::CatppuccinLatte => "Catppuccin Latte".to_string(),
                        },
                        active: name == current,
                    })
                    .collect();
                self.set_theme_picker_options(options);
                self.open_theme_picker();
                true
            }
            SharedUiAction::Skills => {
                self.open_info_panel(InfoPanel::Skills);
                true
            }
        }
    }

    pub fn push_startup_logo(&mut self) {
        use rand::RngExt;
        let idx = rand::rng().random_range(0..STARTUP_LOGOS.len());
        let logo = STARTUP_LOGOS[idx];
        self.push_transcript_item(TranscriptEntry {
            id: 0,
            kind: TranscriptEntryKind::Logo(logo.to_string()),
            status: None,
        });
    }

    pub(crate) fn hydrate_transcript_from_messages(
        &mut self,
        messages: impl IntoIterator<Item = UiMessageSnapshot>,
        last_total_tokens: Option<u64>,
    ) {
        for mut message in messages {
            if let Some(usage) = message.usage() {
                self.hydrate_usage(
                    usage.input_tokens(),
                    usage.output_tokens(),
                    usage.total_tokens(),
                );
            }
            if let Some(display) = message.take_tool_display() {
                append_direct_tool_display(self, display);
                continue;
            }
            let role = match message.role() {
                "user" => TranscriptRole::User,
                "assistant" => TranscriptRole::Assistant,
                "tool" => TranscriptRole::Tool,
                "compaction" => TranscriptRole::Compaction,
                _ => TranscriptRole::System,
            };
            let message_content = message.content();

            if role == TranscriptRole::Compaction {
                self.start_compaction_block("history");
                self.finish_compaction_block("history", CompactionStatus::Done);

                if !message_content.trim().is_empty() {
                    self.push_transcript_item(TranscriptEntry {
                        id: 0,
                        kind: TranscriptEntryKind::Assistant(ProseMessage {
                            markdown: crate::markdown::unwrap_single_fenced_block(message_content),
                        }),
                        status: None,
                    });
                }
                self.push_spacer();
                continue;
            }

            if message_content.trim().is_empty() {
                continue;
            }
            if role == TranscriptRole::Assistant {
                self.push_hydrate_block_start_spacers(role);
                self.push_transcript_item(TranscriptEntry {
                    id: 0,
                    kind: TranscriptEntryKind::Assistant(ProseMessage {
                        markdown: message_content.trim().to_string(),
                    }),
                    status: None,
                });
                self.push_spacer();
                continue;
            }

            if role == TranscriptRole::Tool {
                let persisted = message_content.trim();
                if let Some(arguments) = message.tool_arguments() {
                    let success = message.tool_success().unwrap_or(true);
                    let name = message
                        .tool_name()
                        .unwrap_or_else(|| extract_tool_name(persisted));
                    self.push_hydrate_tool_block_start_spacers();
                    self.start_tool_call(name, arguments);
                    self.finish_tool_call(name, arguments, success);
                    continue;
                }
                if let Some((name, arguments, success)) =
                    parse_persisted_tool_status_line(persisted)
                {
                    self.push_hydrate_tool_block_start_spacers();
                    self.start_tool_call(name, arguments);
                    self.finish_tool_call(name, arguments, success);
                    continue;
                }
                continue;
            }

            self.push_hydrate_block_start_spacers(role);
            for line in message_content.lines() {
                if !line.trim().is_empty() {
                    self.push_transcript_line(role, line.to_string());
                }
            }
            self.push_spacer();
        }

        if self.hydrate_tool_block_is_open() {
            self.push_spacer();
        }

        if let Some(tokens) = last_total_tokens {
            self.hydrate_latest_total_tokens(tokens);
        }
    }

    fn push_hydrate_block_start_spacers(&mut self, role: TranscriptRole) {
        let last_content = self
            .transcript_preview
            .iter()
            .rev()
            .find(|e| !matches!(e.kind, TranscriptEntryKind::Spacer(_)))
            .map(|e| e.role());
        let prev_is_tool_block = matches!(last_content, Some(Role::Tool) | Some(Role::ToolDisplay));

        if role == TranscriptRole::Assistant && prev_is_tool_block {
            self.push_spacer();
            return;
        }

        let prev_is_spacer = self
            .transcript_preview
            .last()
            .is_some_and(|last| matches!(last.kind, TranscriptEntryKind::Spacer(_)));
        if !self.transcript_preview.is_empty() && !prev_is_spacer {
            self.push_spacer();
        }
        self.push_spacer();
    }

    fn push_hydrate_tool_block_start_spacers(&mut self) {
        if self.hydrate_tool_block_is_open() {
            return;
        }
        let last_content = self
            .transcript_preview
            .iter()
            .rev()
            .find(|e| !matches!(e.kind, TranscriptEntryKind::Spacer(_)))
            .map(|e| e.role());
        let prev_is_assistant = matches!(last_content, Some(Role::Assistant));

        if prev_is_assistant {
            let prev_is_spacer = self
                .transcript_preview
                .last()
                .is_some_and(|last| matches!(last.kind, TranscriptEntryKind::Spacer(_)));
            if !prev_is_spacer {
                self.push_spacer();
            }
            return;
        }

        self.push_hydrate_block_start_spacers(TranscriptRole::Tool);
    }

    fn hydrate_tool_block_is_open(&self) -> bool {
        self.transcript_preview.last().is_some_and(|last| {
            matches!(
                last.kind,
                TranscriptEntryKind::Tool(_) | TranscriptEntryKind::ToolResult(_)
            )
        })
    }
}
